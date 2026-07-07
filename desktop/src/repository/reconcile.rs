//! Implementação SQLite do `ReconcileRepository` (anti-entropia — §7).
//!
//! Dinheiro/datas no SQLite são TEXT; aqui só lidamos com `id` (TEXT UUID),
//! `updated_at` (TEXT) e `deleted_at` (TEXT nullable). O nome da tabela é
//! validado contra a allowlist antes de interpolar (sem injeção).

use async_trait::async_trait;
use sqlx::SqlitePool;
use uuid::Uuid;

use letaf_core::error::CoreError;
use letaf_core::reconcile::{is_reconcilable, ManifestEntry, ReconcileRepository};

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
        let sql = format!(
            "SELECT id, updated_at, deleted_at FROM {table} WHERE company_id = ?1"
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
        let sql = format!(
            "UPDATE {table} SET synced = 0 WHERE company_id = ?1 AND id IN ({placeholders})"
        );
        let mut q = sqlx::query(&sql).bind(company_id.to_string());
        for id in ids {
            q = q.bind(id.to_string());
        }
        q.execute(&self.pool).await.map_err(map_db)?;
        Ok(())
    }
}
