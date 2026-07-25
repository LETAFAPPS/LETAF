use async_trait::async_trait;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::PgPool;
use uuid::Uuid;

use letaf_core::admin_role::model::AdminRole;
use letaf_core::admin_role::repository::AdminRoleRepository;
use letaf_core::error::CoreError;

use super::helpers::map_db;

#[derive(FromRow)]
struct AdminRoleRow {
    id: Uuid,
    name: String,
    /// CSV das chaves de tela.
    screens: String,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    deleted_at: Option<NaiveDateTime>,
}

impl From<AdminRoleRow> for AdminRole {
    fn from(r: AdminRoleRow) -> Self {
        let screens = r
            .screens
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        Self {
            id: r.id,
            name: r.name,
            screens,
            created_at: r.created_at,
            updated_at: r.updated_at,
            deleted_at: r.deleted_at,
        }
    }
}

pub struct PgAdminRoleRepository {
    pool: PgPool,
}

impl PgAdminRoleRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AdminRoleRepository for PgAdminRoleRepository {
    async fn find_all(&self) -> Result<Vec<AdminRole>, CoreError> {
        let rows = sqlx::query_as::<_, AdminRoleRow>(
            "SELECT * FROM admin_roles WHERE deleted_at IS NULL ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(AdminRole::from).collect())
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<AdminRole>, CoreError> {
        let row = sqlx::query_as::<_, AdminRoleRow>(
            "SELECT * FROM admin_roles WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(row.map(AdminRole::from))
    }

    async fn create(&self, role: &AdminRole) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO admin_roles (id, name, screens, created_at, updated_at, deleted_at)
             VALUES ($1,$2,$3,$4,$5,$6)",
        )
        .bind(role.id)
        .bind(&role.name)
        .bind(role.screens.join(","))
        .bind(role.created_at)
        .bind(role.updated_at)
        .bind(role.deleted_at)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn update(&self, role: &AdminRole) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE admin_roles SET name = $1, screens = $2, updated_at = $3 WHERE id = $4",
        )
        .bind(&role.name)
        .bind(role.screens.join(","))
        .bind(role.updated_at)
        .bind(role.id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn soft_delete(&self, id: Uuid) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        sqlx::query("UPDATE admin_roles SET deleted_at = $1, updated_at = $1 WHERE id = $2 AND deleted_at IS NULL")
            .bind(now)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(map_db)?;
        // Remove as atribuições da função excluída.
        sqlx::query("DELETE FROM admin_user_roles WHERE role_id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(map_db)?;
        Ok(())
    }

    async fn count_users(&self, role_id: Uuid) -> Result<i64, CoreError> {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM admin_user_roles WHERE role_id = $1")
            .bind(role_id)
            .fetch_one(&self.pool)
            .await
            .map_err(map_db)?;
        Ok(row.0)
    }

    async fn set_user_role(&self, user_id: Uuid, role_id: Option<Uuid>) -> Result<(), CoreError> {
        match role_id {
            Some(rid) => {
                sqlx::query(
                    "INSERT INTO admin_user_roles (user_id, role_id) VALUES ($1,$2)
                     ON CONFLICT (user_id) DO UPDATE SET role_id = EXCLUDED.role_id",
                )
                .bind(user_id)
                .bind(rid)
                .execute(&self.pool)
                .await
                .map_err(map_db)?;
            }
            None => {
                sqlx::query("DELETE FROM admin_user_roles WHERE user_id = $1")
                    .bind(user_id)
                    .execute(&self.pool)
                    .await
                    .map_err(map_db)?;
            }
        }
        Ok(())
    }

    async fn role_for_user(&self, user_id: Uuid) -> Result<Option<AdminRole>, CoreError> {
        let row = sqlx::query_as::<_, AdminRoleRow>(
            "SELECT r.* FROM admin_roles r
             JOIN admin_user_roles m ON m.role_id = r.id
             WHERE m.user_id = $1 AND r.deleted_at IS NULL",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(row.map(AdminRole::from))
    }

    async fn roles_of_users(&self, user_ids: &[Uuid]) -> Result<Vec<(Uuid, Uuid)>, CoreError> {
        if user_ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows: Vec<(Uuid, Uuid)> = sqlx::query_as(
            "SELECT user_id, role_id FROM admin_user_roles WHERE user_id = ANY($1)",
        )
        .bind(user_ids)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows)
    }

    async fn set_user_active(&self, user_id: Uuid, active: bool) -> Result<(), CoreError> {
        sqlx::query("UPDATE admin_user_roles SET active = $2 WHERE user_id = $1")
            .bind(user_id)
            .bind(active)
            .execute(&self.pool)
            .await
            .map_err(map_db)?;
        Ok(())
    }

    async fn is_user_active(&self, user_id: Uuid) -> Result<bool, CoreError> {
        let row: Option<(bool,)> =
            sqlx::query_as("SELECT active FROM admin_user_roles WHERE user_id = $1")
                .bind(user_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(map_db)?;
        // Sem linha (master) = sempre ativo.
        Ok(row.map(|(a,)| a).unwrap_or(true))
    }

    async fn active_of_users(&self, user_ids: &[Uuid]) -> Result<Vec<(Uuid, bool)>, CoreError> {
        if user_ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows: Vec<(Uuid, bool)> = sqlx::query_as(
            "SELECT user_id, active FROM admin_user_roles WHERE user_id = ANY($1)",
        )
        .bind(user_ids)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows)
    }
}
