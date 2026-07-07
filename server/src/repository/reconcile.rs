//! Implementação PostgreSQL do `ReconcileRepository` (anti-entropia — §7).
//!
//! Query genérica por tabela: como todas as entidades de `RECONCILE_TABLES`
//! compartilham `id`/`company_id`/`updated_at`/`deleted_at`/`synced`, um único
//! repositório serve a todas. O nome da tabela é validado contra a allowlist
//! (`is_reconcilable`) ANTES de ser interpolado — sem risco de injeção.

use async_trait::async_trait;
use chrono::NaiveDateTime;
use sqlx::PgPool;
use uuid::Uuid;

use letaf_core::error::CoreError;
use letaf_core::reconcile::{is_reconcilable, ManifestEntry, ReconcileRepository};

use super::helpers::map_db;

pub struct PgReconcileRepository {
    pool: PgPool,
}

impl PgReconcileRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ReconcileRepository for PgReconcileRepository {
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
        // `table` é da allowlist — seguro interpolar. Filtro por tenant (§11).
        let sql = format!(
            "SELECT id, updated_at, deleted_at FROM {table} WHERE company_id = $1"
        );
        let rows: Vec<(Uuid, NaiveDateTime, Option<NaiveDateTime>)> =
            sqlx::query_as(&sql)
                .bind(company_id)
                .fetch_all(&self.pool)
                .await
                .map_err(map_db)?;
        Ok(rows
            .into_iter()
            .map(|(id, updated_at, deleted_at)| ManifestEntry { id, updated_at, deleted_at })
            .collect())
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
        let sql = format!(
            "UPDATE {table} SET synced = false WHERE company_id = $1 AND id = ANY($2)"
        );
        sqlx::query(&sql)
            .bind(company_id)
            .bind(ids)
            .execute(&self.pool)
            .await
            .map_err(map_db)?;
        Ok(())
    }
}
