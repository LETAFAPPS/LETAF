use async_trait::async_trait;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::SqlitePool;
use uuid::Uuid;

use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;
use letaf_core::job_role::model::JobRole;
use letaf_core::job_role::repository::JobRoleRepository;

use super::helpers::{map_db, parse_timestamp, parse_uuid, ts};

#[derive(FromRow)]
struct JobRoleRow {
    id: String,
    company_id: String,
    name: String,
    permissions: String,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    synced: bool,
}

impl TryFrom<JobRoleRow> for JobRole {
    type Error = CoreError;

    fn try_from(r: JobRoleRow) -> Result<Self, Self::Error> {
        Ok(Self {
            base: BaseFields {
                id: parse_uuid(&r.id)?,
                company_id: parse_uuid(&r.company_id)?,
                created_at: parse_timestamp(&r.created_at)?,
                updated_at: parse_timestamp(&r.updated_at)?,
                deleted_at: r.deleted_at.as_deref().map(parse_timestamp).transpose()?,
                synced: r.synced,
            },
            name: r.name,
            permissions: serde_json::from_str(&r.permissions).unwrap_or_default(),
        })
    }
}

fn perms_json(perms: &[String]) -> String {
    serde_json::to_string(perms).unwrap_or_else(|_| "[]".into())
}

/// Implementação SQLite do JobRoleRepository (desktop, offline-first).
pub struct SqliteJobRoleRepository {
    pool: SqlitePool,
}

impl SqliteJobRoleRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl JobRoleRepository for SqliteJobRoleRepository {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<JobRole>, CoreError> {
        let row = sqlx::query_as::<_, JobRoleRow>(
            "SELECT * FROM job_roles WHERE company_id = ?1 AND id = ?2 AND deleted_at IS NULL",
        )
        .bind(company_id.to_string())
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        row.map(JobRole::try_from).transpose()
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<JobRole>, CoreError> {
        let rows = sqlx::query_as::<_, JobRoleRow>(
            "SELECT * FROM job_roles WHERE company_id = ?1 AND deleted_at IS NULL ORDER BY name ASC",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(JobRole::try_from).collect()
    }

    async fn create(&self, role: &JobRole) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO job_roles (id, company_id, name, permissions, created_at, updated_at, deleted_at, synced)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )
        .bind(role.base.id.to_string())
        .bind(role.base.company_id.to_string())
        .bind(&role.name)
        .bind(perms_json(&role.permissions))
        .bind(ts(role.base.created_at))
        .bind(ts(role.base.updated_at))
        .bind(role.base.deleted_at.map(ts))
        .bind(role.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn update(&self, role: &JobRole) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE job_roles SET name = ?1, permissions = ?2, updated_at = ?3, synced = ?4
             WHERE company_id = ?5 AND id = ?6 AND deleted_at IS NULL",
        )
        .bind(&role.name)
        .bind(perms_json(&role.permissions))
        .bind(ts(role.base.updated_at))
        .bind(role.base.synced)
        .bind(role.base.company_id.to_string())
        .bind(role.base.id.to_string())
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let now = ts(chrono::Utc::now().naive_utc());
        sqlx::query(
            "UPDATE job_roles SET deleted_at = ?1, updated_at = ?2, synced = 0
             WHERE company_id = ?3 AND id = ?4 AND deleted_at IS NULL",
        )
        .bind(&now)
        .bind(&now)
        .bind(company_id.to_string())
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<JobRole>, CoreError> {
        let rows = sqlx::query_as::<_, JobRoleRow>(
            "SELECT * FROM job_roles WHERE company_id = ?1 AND synced = 0",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(JobRole::try_from).collect()
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        sqlx::query("UPDATE job_roles SET synced = 1 WHERE company_id = ?1 AND id = ?2")
            .bind(company_id.to_string())
            .bind(id.to_string())
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
            "SELECT * FROM job_roles WHERE company_id = ?1 AND updated_at > ?2",
        )
        .bind(company_id.to_string())
        .bind(ts(since))
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(JobRole::try_from).collect()
    }

    async fn sync_upsert(&self, role: &JobRole) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO job_roles (id, company_id, name, permissions, created_at, updated_at, deleted_at, synced)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (id) DO UPDATE SET
                 name = excluded.name,
                 permissions = excluded.permissions,
                 updated_at = excluded.updated_at,
                 deleted_at = excluded.deleted_at,
                 synced = excluded.synced
             WHERE excluded.updated_at > job_roles.updated_at",
        )
        .bind(role.base.id.to_string())
        .bind(role.base.company_id.to_string())
        .bind(&role.name)
        .bind(perms_json(&role.permissions))
        .bind(ts(role.base.created_at))
        .bind(ts(role.base.updated_at))
        .bind(role.base.deleted_at.map(ts))
        .bind(role.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }
}
