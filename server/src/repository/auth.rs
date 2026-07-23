use async_trait::async_trait;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::PgPool;
use uuid::Uuid;

use letaf_core::auth::model::{User, UserRole};
use letaf_core::auth::repository::UserRepository;
use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;

use super::helpers::map_db;

#[derive(FromRow)]
struct UserRow {
    id: Uuid,
    company_id: Uuid,
    email: String,
    password_hash: String,
    name: String,
    role: String,
    job_role_id: Option<Uuid>,
    avatar: Option<String>,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    deleted_at: Option<NaiveDateTime>,
    synced: bool,
}

impl From<UserRow> for User {
    fn from(r: UserRow) -> Self {
        // Valor desconhecido cai em Admin (default seguro do dono — §11).
        let role = UserRole::from_db_str(&r.role).unwrap_or_else(|| {
            tracing::warn!("Role desconhecida no banco: {:?} (user id={}); usando Admin", r.role, r.id);
            UserRole::Admin
        });
        Self {
            base: BaseFields {
                id: r.id,
                company_id: r.company_id,
                created_at: r.created_at,
                updated_at: r.updated_at,
                deleted_at: r.deleted_at,
                synced: r.synced,
            },
            email: r.email,
            password_hash: r.password_hash,
            name: r.name,
            role,
            job_role_id: r.job_role_id,
            avatar: r.avatar,
        }
    }
}

/// Implementação PostgreSQL do UserRepository.
///
/// Regras aplicadas (AI_RULES.md §3, §5, §6, §10, §11):
/// - Todas queries filtram por company_id (isolamento)
/// - Soft delete via deleted_at
/// - Servidor usa PostgreSQL
/// - Acesso ao banco somente via repository
/// - Preparar autenticação
pub struct PgUserRepository {
    pool: PgPool,
}

impl PgUserRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UserRepository for PgUserRepository {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<User>, CoreError> {
        sqlx::query_as::<_, UserRow>(
            "SELECT * FROM users WHERE company_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(company_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map(|opt| opt.map(User::from))
        .map_err(map_db)
    }

    async fn find_by_email(&self, company_id: Uuid, email: &str) -> Result<Option<User>, CoreError> {
        sqlx::query_as::<_, UserRow>(
            "SELECT * FROM users WHERE company_id = $1 AND email = $2 AND deleted_at IS NULL",
        )
        .bind(company_id)
        .bind(email)
        .fetch_optional(&self.pool)
        .await
        .map(|opt| opt.map(User::from))
        .map_err(map_db)
    }

    async fn find_by_email_any(&self, company_id: Uuid, email: &str) -> Result<Option<User>, CoreError> {
        sqlx::query_as::<_, UserRow>(
            "SELECT * FROM users WHERE company_id = $1 AND email = $2 LIMIT 1",
        )
        .bind(company_id)
        .bind(email)
        .fetch_optional(&self.pool)
        .await
        .map(|opt| opt.map(User::from))
        .map_err(map_db)
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<User>, CoreError> {
        let rows = sqlx::query_as::<_, UserRow>(
            "SELECT * FROM users WHERE company_id = $1 AND deleted_at IS NULL ORDER BY name",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(rows.into_iter().map(User::from).collect())
    }

    async fn create(&self, user: &User) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO users (id, company_id, email, password_hash, name, role, job_role_id, avatar, created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
        )
        .bind(user.base.id)
        .bind(user.base.company_id)
        .bind(&user.email)
        .bind(&user.password_hash)
        .bind(&user.name)
        .bind(user.role.as_db_str())
        .bind(user.job_role_id)
        .bind(&user.avatar)
        .bind(user.base.created_at)
        .bind(user.base.updated_at)
        .bind(user.base.deleted_at)
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
            "UPDATE users SET email = $1, name = $2, password_hash = $3, role = $4, job_role_id = $5, avatar = $6, updated_at = $7, synced = $8, deleted_at = $9
             WHERE company_id = $10 AND id = $11",
        )
        .bind(&user.email)
        .bind(&user.name)
        .bind(&user.password_hash)
        .bind(user.role.as_db_str())
        .bind(user.job_role_id)
        .bind(&user.avatar)
        .bind(user.base.updated_at)
        .bind(user.base.synced)
        .bind(user.base.deleted_at)
        .bind(user.base.company_id)
        .bind(user.base.id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        sqlx::query(
            "UPDATE users SET deleted_at = $1, updated_at = $2, synced = false
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

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<User>, CoreError> {
        let rows = sqlx::query_as::<_, UserRow>(
            "SELECT * FROM users WHERE company_id = $1 AND synced = false",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(rows.into_iter().map(User::from).collect())
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query("UPDATE users SET synced = true WHERE company_id = $1 AND id = $2 AND updated_at = $3")
            .bind(company_id)
            .bind(id)
        .bind(updated_at)
            .execute(&self.pool)
            .await
            .map_err(map_db)?;

        Ok(())
    }

    async fn find_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<User>, CoreError> {
        let rows = sqlx::query_as::<_, UserRow>(
            "SELECT * FROM users WHERE company_id = $1 AND updated_at > $2",
        )
        .bind(company_id)
        .bind(since)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(rows.into_iter().map(User::from).collect())
    }

    /// Busca usuário por e-mail SEM filtro de tenant (apenas para o
    /// login do desktop, antes de o cliente conhecer seu subdomínio).
    ///
    /// Regras aplicadas (AI_RULES.md §11):
    /// - Hoje a coluna `email` não é UNIQUE global — dois tenants
    ///   podem ter o mesmo email cadastrado (cada um com sua senha).
    ///   Se houver mais de um match, RECUSA o login: escolher LIMIT 1
    ///   silenciosamente poderia autenticar o operador no tenant
    ///   errado, vazando dados entre empresas. O fluxo correto nesse
    ///   caso é o operador pedir suporte para desambiguar.
    async fn find_by_email_global(&self, email: &str) -> Result<Option<User>, CoreError> {
        let rows = sqlx::query_as::<_, UserRow>(
            "SELECT * FROM users WHERE email = $1 AND deleted_at IS NULL LIMIT 2",
        )
        .bind(email)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        if rows.len() > 1 {
            return Err(CoreError::Validation(
                "Email cadastrado em mais de uma empresa — contate o suporte".into(),
            ));
        }
        Ok(rows.into_iter().next().map(User::from))
    }

    async fn find_token_version(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<Option<i32>, CoreError> {
        // Uma query cobre existência (None = inexistente/soft-deletado) e a
        // versão de credencial (RBAC §11).
        let row: Option<(i32,)> = sqlx::query_as(
            "SELECT token_version FROM users
              WHERE company_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(company_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(row.map(|(v,)| v))
    }

    async fn bump_token_version(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE users SET token_version = token_version + 1
              WHERE company_id = $1 AND id = $2",
        )
        .bind(company_id)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn bump_token_version_by_job_role(
        &self,
        company_id: Uuid,
        job_role_id: Uuid,
    ) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE users SET token_version = token_version + 1
              WHERE company_id = $1 AND job_role_id = $2",
        )
        .bind(company_id)
        .bind(job_role_id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn sync_upsert(&self, user: &User) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO users (id, company_id, email, password_hash, name, role, job_role_id, avatar, created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
             ON CONFLICT (id) DO UPDATE SET
                 email = EXCLUDED.email,
                 password_hash = EXCLUDED.password_hash,
                 name = EXCLUDED.name,
                 role = EXCLUDED.role,
                 job_role_id = EXCLUDED.job_role_id,
                 avatar = EXCLUDED.avatar,
                 updated_at = EXCLUDED.updated_at,
                 deleted_at = EXCLUDED.deleted_at,
                 synced = EXCLUDED.synced
             WHERE EXCLUDED.updated_at > users.updated_at AND users.company_id = EXCLUDED.company_id",
        )
        .bind(user.base.id)
        .bind(user.base.company_id)
        .bind(&user.email)
        .bind(&user.password_hash)
        .bind(&user.name)
        .bind(user.role.as_db_str())
        .bind(user.job_role_id)
        .bind(&user.avatar)
        .bind(user.base.created_at)
        .bind(user.base.updated_at)
        .bind(user.base.deleted_at)
        .bind(user.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }
}
