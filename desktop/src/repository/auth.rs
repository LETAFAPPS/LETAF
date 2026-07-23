use async_trait::async_trait;
use sqlx::prelude::FromRow;
use sqlx::SqlitePool;
use uuid::Uuid;

use letaf_core::auth::model::{User, UserRole};
use letaf_core::auth::repository::UserRepository;
use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;

use super::helpers::{map_db, parse_timestamp, parse_uuid, ts};

#[derive(FromRow)]
struct UserRow {
    id: String,
    company_id: String,
    email: String,
    password_hash: String,
    name: String,
    role: String,
    job_role_id: Option<String>,
    avatar: Option<String>,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    synced: bool,
}

impl TryFrom<UserRow> for User {
    type Error = CoreError;

    fn try_from(r: UserRow) -> Result<Self, Self::Error> {
        // Valor desconhecido cai em Admin (default seguro do dono — §11).
        let id = parse_uuid(&r.id)?;
        let role = UserRole::from_db_str(&r.role).unwrap_or_else(|| {
            tracing::warn!("Role desconhecida no banco: {:?} (user id={}); usando Admin", r.role, id);
            UserRole::Admin
        });
        Ok(Self {
            base: BaseFields {
                id,
                company_id: parse_uuid(&r.company_id)?,
                created_at: parse_timestamp(&r.created_at)?,
                updated_at: parse_timestamp(&r.updated_at)?,
                deleted_at: r.deleted_at.as_deref().map(parse_timestamp).transpose()?,
                synced: r.synced,
            },
            email: r.email,
            password_hash: r.password_hash,
            name: r.name,
            role,
            job_role_id: r.job_role_id.as_deref().map(parse_uuid).transpose()?,
            avatar: r.avatar,
        })
    }
}

/// Implementação SQLite do UserRepository.
///
/// Regras aplicadas (AI_RULES.md §3, §5, §7, §10, §11):
/// - Desktop usa SQLite
/// - Todas queries filtram por company_id (isolamento)
/// - Soft delete via deleted_at
/// - Offline-first
/// - Acesso ao banco somente via repository
pub struct SqliteUserRepository {
    pool: SqlitePool,
}

impl SqliteUserRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UserRepository for SqliteUserRepository {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<User>, CoreError> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT * FROM users WHERE company_id = ?1 AND id = ?2 AND deleted_at IS NULL",
        )
        .bind(company_id.to_string())
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;

        row.map(User::try_from).transpose()
    }

    async fn find_by_email(&self, company_id: Uuid, email: &str) -> Result<Option<User>, CoreError> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT * FROM users WHERE company_id = ?1 AND email = ?2 AND deleted_at IS NULL",
        )
        .bind(company_id.to_string())
        .bind(email)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;

        row.map(User::try_from).transpose()
    }

    async fn find_by_email_any(&self, company_id: Uuid, email: &str) -> Result<Option<User>, CoreError> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT * FROM users WHERE company_id = ?1 AND email = ?2 LIMIT 1",
        )
        .bind(company_id.to_string())
        .bind(email)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;

        row.map(User::try_from).transpose()
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<User>, CoreError> {
        let rows = sqlx::query_as::<_, UserRow>(
            "SELECT * FROM users WHERE company_id = ?1 AND deleted_at IS NULL ORDER BY name",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        rows.into_iter().map(User::try_from).collect()
    }

    async fn create(&self, user: &User) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO users (id, company_id, email, password_hash, name, role, job_role_id, avatar, created_at, updated_at, deleted_at, synced)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        )
        .bind(user.base.id.to_string())
        .bind(user.base.company_id.to_string())
        .bind(&user.email)
        .bind(&user.password_hash)
        .bind(&user.name)
        .bind(user.role.as_db_str())
        .bind(user.job_role_id.map(|id| id.to_string()))
        .bind(&user.avatar)
        .bind(ts(user.base.created_at))
        .bind(ts(user.base.updated_at))
        .bind(user.base.deleted_at.map(ts))
        .bind(user.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    async fn update(&self, user: &User) -> Result<(), CoreError> {
        // Grava `deleted_at` e não filtra por ele no WHERE: além do update
        // comum (registro ativo → deleted_at NULL, no-op), permite REATIVAR
        // um funcionário excluído ao recriar com o mesmo e-mail.
        sqlx::query(
            "UPDATE users SET email = ?1, name = ?2, password_hash = ?3, role = ?4, job_role_id = ?5, avatar = ?6, updated_at = ?7, synced = ?8, deleted_at = ?9
             WHERE company_id = ?10 AND id = ?11",
        )
        .bind(&user.email)
        .bind(&user.name)
        .bind(&user.password_hash)
        .bind(user.role.as_db_str())
        .bind(user.job_role_id.map(|id| id.to_string()))
        .bind(&user.avatar)
        .bind(ts(user.base.updated_at))
        .bind(user.base.synced)
        .bind(user.base.deleted_at.map(ts))
        .bind(user.base.company_id.to_string())
        .bind(user.base.id.to_string())
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let now = ts(chrono::Utc::now().naive_utc());
        sqlx::query(
            "UPDATE users SET deleted_at = ?1, updated_at = ?2, synced = false
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

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<User>, CoreError> {
        let rows = sqlx::query_as::<_, UserRow>(
            "SELECT * FROM users WHERE company_id = ?1 AND synced = false",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        rows.into_iter().map(User::try_from).collect()
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query("UPDATE users SET synced = true WHERE company_id = ?1 AND id = ?2 AND updated_at = ?3")
            .bind(company_id.to_string())
            .bind(id.to_string())
            .bind(ts(updated_at))
            .execute(&self.pool)
            .await
            .map_err(map_db)?;

        Ok(())
    }

    async fn find_updated_since(&self, company_id: Uuid, since: chrono::NaiveDateTime) -> Result<Vec<User>, CoreError> {
        let rows = sqlx::query_as::<_, UserRow>(
            "SELECT * FROM users WHERE company_id = ?1 AND updated_at > ?2",
        )
        .bind(company_id.to_string())
        .bind(ts(since))
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        rows.into_iter().map(User::try_from).collect()
    }

    async fn find_by_email_global(&self, email: &str) -> Result<Option<User>, CoreError> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT * FROM users WHERE email = ?1 AND deleted_at IS NULL LIMIT 1",
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;

        row.map(User::try_from).transpose()
    }

    async fn sync_upsert(&self, user: &User) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO users (id, company_id, email, password_hash, name, role, job_role_id, avatar, created_at, updated_at, deleted_at, synced)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT (id) DO UPDATE SET
                 email = excluded.email,
                 password_hash = excluded.password_hash,
                 name = excluded.name,
                 role = excluded.role,
                 job_role_id = excluded.job_role_id,
                 avatar = excluded.avatar,
                 updated_at = excluded.updated_at,
                 deleted_at = excluded.deleted_at,
                 synced = excluded.synced
             WHERE excluded.updated_at > users.updated_at",
        )
        .bind(user.base.id.to_string())
        .bind(user.base.company_id.to_string())
        .bind(&user.email)
        .bind(&user.password_hash)
        .bind(&user.name)
        .bind(user.role.as_db_str())
        .bind(user.job_role_id.map(|id| id.to_string()))
        .bind(&user.avatar)
        .bind(ts(user.base.created_at))
        .bind(ts(user.base.updated_at))
        .bind(user.base.deleted_at.map(ts))
        .bind(user.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }
}
