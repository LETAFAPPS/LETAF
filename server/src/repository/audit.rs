use async_trait::async_trait;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::PgPool;
use uuid::Uuid;

use letaf_core::audit::model::AuditEntry;
use letaf_core::audit::repository::AuditRepository;
use letaf_core::error::CoreError;

use super::helpers::map_db;

#[derive(FromRow)]
struct AuditRow {
    id: Uuid,
    actor_id: Uuid,
    actor_name: String,
    action: String,
    target_type: String,
    target_id: Option<Uuid>,
    target_label: String,
    details: String,
    created_at: NaiveDateTime,
}

impl From<AuditRow> for AuditEntry {
    fn from(r: AuditRow) -> Self {
        Self {
            id: r.id,
            actor_id: r.actor_id,
            actor_name: r.actor_name,
            action: r.action,
            target_type: r.target_type,
            target_id: r.target_id,
            target_label: r.target_label,
            details: r.details,
            created_at: r.created_at,
        }
    }
}

/// Implementação PostgreSQL da trilha de auditoria.
///
/// Regras aplicadas (AI_RULES.md §5, §10, §11):
/// - Servidor usa PostgreSQL; acesso só via repository.
/// - Trilha imutável: expõe apenas INSERT e SELECT.
pub struct PgAuditRepository {
    pool: PgPool,
}

impl PgAuditRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AuditRepository for PgAuditRepository {
    async fn create(&self, entry: &AuditEntry) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO admin_audit_log
                (id, actor_id, actor_name, action, target_type, target_id, target_label, details, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(entry.id)
        .bind(entry.actor_id)
        .bind(&entry.actor_name)
        .bind(&entry.action)
        .bind(&entry.target_type)
        .bind(entry.target_id)
        .bind(&entry.target_label)
        .bind(&entry.details)
        .bind(entry.created_at)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    async fn find_recent(&self, limit: i64) -> Result<Vec<AuditEntry>, CoreError> {
        let rows = sqlx::query_as::<_, AuditRow>(
            "SELECT * FROM admin_audit_log ORDER BY created_at DESC LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(rows.into_iter().map(AuditEntry::from).collect())
    }
}
