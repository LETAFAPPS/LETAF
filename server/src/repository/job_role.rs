use async_trait::async_trait;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::PgPool;
use uuid::Uuid;

use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;
use letaf_core::job_role::model::JobRole;
use letaf_core::job_role::repository::JobRoleRepository;

use super::helpers::map_db;

#[derive(FromRow)]
struct JobRoleRow {
    id: Uuid,
    company_id: Uuid,
    name: String,
    /// JSON array de chaves de permissão.
    permissions: String,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    deleted_at: Option<NaiveDateTime>,
    synced: bool,
}

impl From<JobRoleRow> for JobRole {
    fn from(r: JobRoleRow) -> Self {
        Self {
            base: BaseFields {
                id: r.id,
                company_id: r.company_id,
                created_at: r.created_at,
                updated_at: r.updated_at,
                deleted_at: r.deleted_at,
                synced: r.synced,
            },
            name: r.name,
            permissions: serde_json::from_str(&r.permissions).unwrap_or_default(),
        }
    }
}

/// Serializa as permissões para a coluna TEXT (JSON).
fn perms_json(perms: &[String]) -> String {
    serde_json::to_string(perms).unwrap_or_else(|_| "[]".into())
}

/// Implementação PostgreSQL do JobRoleRepository (AI_RULES §3,§5,§6,§10):
/// queries filtram por company_id, soft delete, acesso só via repository.
pub struct PgJobRoleRepository {
    pool: PgPool,
}

impl PgJobRoleRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl JobRoleRepository for PgJobRoleRepository {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<JobRole>, CoreError> {
        sqlx::query_as::<_, JobRoleRow>(
            "SELECT * FROM job_roles WHERE company_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(company_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map(|opt| opt.map(JobRole::from))
        .map_err(map_db)
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<JobRole>, CoreError> {
        let rows = sqlx::query_as::<_, JobRoleRow>(
            "SELECT * FROM job_roles WHERE company_id = $1 AND deleted_at IS NULL ORDER BY name ASC",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(JobRole::from).collect())
    }

    async fn create(&self, role: &JobRole) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO job_roles (id, company_id, name, permissions, created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(role.base.id)
        .bind(role.base.company_id)
        .bind(&role.name)
        .bind(perms_json(&role.permissions))
        .bind(role.base.created_at)
        .bind(role.base.updated_at)
        .bind(role.base.deleted_at)
        .bind(role.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn update(&self, role: &JobRole) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE job_roles SET name = $1, permissions = $2, updated_at = $3, synced = $4
             WHERE company_id = $5 AND id = $6 AND deleted_at IS NULL",
        )
        .bind(&role.name)
        .bind(perms_json(&role.permissions))
        .bind(role.base.updated_at)
        .bind(role.base.synced)
        .bind(role.base.company_id)
        .bind(role.base.id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        sqlx::query(
            "UPDATE job_roles SET deleted_at = $1, updated_at = $2, synced = false
             WHERE company_id = $3 AND id = $4 AND deleted_at IS NULL",
        )
        .bind(now)
        .bind(now)
        .bind(company_id)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<JobRole>, CoreError> {
        let rows = sqlx::query_as::<_, JobRoleRow>(
            "SELECT * FROM job_roles WHERE company_id = $1 AND synced = false",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(JobRole::from).collect())
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        sqlx::query("UPDATE job_roles SET synced = true WHERE company_id = $1 AND id = $2")
            .bind(company_id)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(map_db)?;
        Ok(())
    }

    async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
    ) -> Result<Vec<JobRole>, CoreError> {
        let rows = sqlx::query_as::<_, JobRoleRow>(
            "SELECT * FROM job_roles WHERE company_id = $1 AND updated_at > $2",
        )
        .bind(company_id)
        .bind(since)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(JobRole::from).collect())
    }

    async fn sync_upsert(&self, role: &JobRole) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO job_roles (id, company_id, name, permissions, created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (id) DO UPDATE SET
                 name = EXCLUDED.name,
                 permissions = EXCLUDED.permissions,
                 updated_at = EXCLUDED.updated_at,
                 deleted_at = EXCLUDED.deleted_at,
                 synced = EXCLUDED.synced
             WHERE EXCLUDED.updated_at > job_roles.updated_at AND job_roles.company_id = EXCLUDED.company_id",
        )
        .bind(role.base.id)
        .bind(role.base.company_id)
        .bind(&role.name)
        .bind(perms_json(&role.permissions))
        .bind(role.base.created_at)
        .bind(role.base.updated_at)
        .bind(role.base.deleted_at)
        .bind(role.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }
}
