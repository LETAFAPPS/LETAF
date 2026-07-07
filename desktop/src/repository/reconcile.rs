//! Implementação SQLite do `ReconcileRepository` (anti-entropia — §7).
//!
//! Dinheiro/datas no SQLite são TEXT; aqui só lidamos com `id` (TEXT UUID),
//! `updated_at` (TEXT) e `deleted_at` (TEXT nullable). O nome da tabela é
//! validado contra a allowlist antes de interpolar (sem injeção).

use async_trait::async_trait;
use sqlx::SqlitePool;
use uuid::Uuid;

use letaf_core::error::CoreError;
use letaf_core::reconcile::{is_reconcilable, tenant_key_column, ManifestEntry, ReconcileRepository};

use super::helpers::{map_db, parse_timestamp, parse_uuid};

pub struct SqliteReconcileRepository {
    pool: SqlitePool,
}

impl SqliteReconcileRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ReconcileRepository for SqliteReconcileRepository {
    async fn manifest(
        &self,
        company_id: Uuid,
        table: &str,
    ) -> Result<Vec<ManifestEntry>, CoreError> {
        if !is_reconcilable(table) {
            return Err(CoreError::Validation(format!(
                "Entidade não reconciliável: {table}"
            )));
        }
        let key = tenant_key_column(table);
        let sql = format!(
            "SELECT id, updated_at, deleted_at FROM {table} WHERE {key} = ?1"
        );
        let rows: Vec<(String, String, Option<String>)> = sqlx::query_as(&sql)
            .bind(company_id.to_string())
            .fetch_all(&self.pool)
            .await
            .map_err(map_db)?;
        rows.into_iter()
            .map(|(id, updated_at, deleted_at)| {
                Ok(ManifestEntry {
                    id: parse_uuid(&id)?,
                    updated_at: parse_timestamp(&updated_at)?,
                    deleted_at: deleted_at.as_deref().map(parse_timestamp).transpose()?,
                })
            })
            .collect()
    }

    async fn mark_unsynced(
        &self,
        company_id: Uuid,
        table: &str,
        ids: &[Uuid],
    ) -> Result<(), CoreError> {
        if !is_reconcilable(table) {
            return Err(CoreError::Validation(format!(
                "Entidade não reconciliável: {table}"
            )));
        }
        if ids.is_empty() {
            return Ok(());
        }
        // SQLite não tem `= ANY`; monta `IN (?2, ?3, ...)` com placeholders
        // (valores são UUIDs internos — sem risco de injeção).
        let placeholders = (0..ids.len())
            .map(|i| format!("?{}", i + 2))
            .collect::<Vec<_>>()
            .join(", ");
        let key = tenant_key_column(table);
        let sql = format!(
            "UPDATE {table} SET synced = 0 WHERE {key} = ?1 AND id IN ({placeholders})"
        );
        let mut q = sqlx::query(&sql).bind(company_id.to_string());
        for id in ids {
            q = q.bind(id.to_string());
        }
        q.execute(&self.pool).await.map_err(map_db)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use letaf_core::reconcile::diff;
    use sqlx::sqlite::SqlitePoolOptions;

    /// Pool SQLite em memória com as migrations aplicadas. `max_connections=1`
    /// mantém o MESMO banco em memória entre queries (memória é por-conexão).
    async fn mem_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    async fn insert_product(
        pool: &SqlitePool,
        cid: Uuid,
        id: Uuid,
        updated: &str,
        synced: bool,
        deleted: Option<&str>,
    ) {
        sqlx::query(
            "INSERT INTO products (id, company_id, name, price, stock_quantity, min_stock, \
             unlimited_stock, unit, created_at, updated_at, deleted_at, synced, active, \
             web_visible, balance_mode) \
             VALUES (?1, ?2, 'P', NULL, 0, 0, 1, 'un', ?3, ?3, ?4, ?5, 1, 1, 'weight')",
        )
        .bind(id.to_string())
        .bind(cid.to_string())
        .bind(updated)
        .bind(deleted)
        .bind(synced)
        .execute(pool)
        .await
        .unwrap();
    }

    /// Cenário do usuário: dois computadores da mesma empresa divergem. O
    /// "servidor" tem P1+P2; o cliente local só tem P1 (faltando P2) e tem um
    /// P3 que o servidor não tem. A reconciliação deve detectar os DOIS lados.
    #[tokio::test]
    async fn two_clients_diverge_reconcile_detects_both_directions() {
        let cid = Uuid::new_v4();
        let (p1, p2, p3) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let server = mem_pool().await;
        let local = mem_pool().await;
        insert_product(&server, cid, p1, "2026-01-05 12:00:00.000000", true, None).await;
        insert_product(&server, cid, p2, "2026-01-06 12:00:00.000000", true, None).await;
        insert_product(&local, cid, p1, "2026-01-05 12:00:00.000000", true, None).await;
        insert_product(&local, cid, p3, "2026-01-07 12:00:00.000000", true, None).await;

        let server_manifest = SqliteReconcileRepository::new(server)
            .manifest(cid, "products").await.unwrap();
        let lrepo = SqliteReconcileRepository::new(local.clone());
        let local_manifest = lrepo.manifest(cid, "products").await.unwrap();

        let d = diff(&local_manifest, &server_manifest);
        assert!(d.server_drift, "P2 falta no local → deve disparar re-pull");
        assert_eq!(d.push_ids, vec![p3], "P3 só no local → deve re-empurrar");

        // Reparo local→servidor: marca P3 como não-sincronizado.
        lrepo.mark_unsynced(cid, "products", &d.push_ids).await.unwrap();
        let synced: bool = sqlx::query_scalar("SELECT synced FROM products WHERE id = ?1")
            .bind(p3.to_string())
            .fetch_one(&local)
            .await
            .unwrap();
        assert!(!synced, "P3 deve ficar synced=false para o push reenviar");
    }

    #[tokio::test]
    async fn manifest_includes_soft_delete_and_allowlist_rejects_unknown() {
        let cid = Uuid::new_v4();
        let pool = mem_pool().await;
        let id = Uuid::new_v4();
        insert_product(&pool, cid, id, "2026-01-05 12:00:00.000000", true,
            Some("2026-01-06 12:00:00.000000")).await;
        let repo = SqliteReconcileRepository::new(pool);

        let m = repo.manifest(cid, "products").await.unwrap();
        assert_eq!(m.len(), 1);
        assert!(m[0].deleted_at.is_some(), "soft-delete deve aparecer no manifesto");

        // Tabela fora da allowlist é rejeitada (defesa contra injeção).
        assert!(repo.manifest(cid, "hackers").await.is_err());
        assert!(repo.mark_unsynced(cid, "hackers", &[id]).await.is_err());
    }
}
