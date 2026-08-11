use async_trait::async_trait;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::PgPool;
use uuid::Uuid;

use letaf_core::error::CoreError;
use letaf_core::password_reset::model::PasswordReset;
use letaf_core::password_reset::repository::PasswordResetRepository;

use super::helpers::map_db;

#[derive(FromRow)]
struct ResetRow {
    id: Uuid,
    email: String,
    code_hash: String,
    expires_at: NaiveDateTime,
    used: bool,
    created_at: NaiveDateTime,
}

impl From<ResetRow> for PasswordReset {
    fn from(r: ResetRow) -> Self {
        Self {
            id: r.id,
            email: r.email,
            code_hash: r.code_hash,
            expires_at: r.expires_at,
            used: r.used,
            created_at: r.created_at,
        }
    }
}

pub struct PgPasswordResetRepository {
    pool: PgPool,
}

impl PgPasswordResetRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl PasswordResetRepository for PgPasswordResetRepository {
    async fn create(&self, reset: &PasswordReset) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO password_resets (id, email, code_hash, expires_at, used, created_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(reset.id)
        .bind(&reset.email)
        .bind(&reset.code_hash)
        .bind(reset.expires_at)
        .bind(reset.used)
        .bind(reset.created_at)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn find_active(&self, email: &str) -> Result<Option<PasswordReset>, CoreError> {
        let row = sqlx::query_as::<_, ResetRow>(
            "SELECT * FROM password_resets
             WHERE email = $1 AND used = FALSE
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(row.map(PasswordReset::from))
    }

    async fn mark_used(&self, id: Uuid) -> Result<bool, CoreError> {
        // Consumo ATÔMICO: só marca se ainda `used = FALSE`. `rows_affected`
        // distingue quem venceu a corrida (1) de um consumo concorrente (0).
        let res = sqlx::query("UPDATE password_resets SET used = TRUE WHERE id = $1 AND used = FALSE")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(map_db)?;
        Ok(res.rows_affected() == 1)
    }

    async fn claim_verify_attempt(&self, id: Uuid, max_attempts: i32) -> Result<bool, CoreError> {
        // Consome um slot ATOMICAMENTE: só incrementa se ainda ativo e abaixo do
        // teto. O `WHERE attempts < $2` bloqueia o (max+1)-ésimo palpite, então o
        // código nunca é pré-marcado `used` — um acerto na última tentativa ainda
        // é consumido normalmente por `mark_used`. O lock de linha do UPDATE
        // serializa rajadas concorrentes (só os primeiros `max` chegam ao bcrypt).
        let res = sqlx::query(
            "UPDATE password_resets
             SET attempts = attempts + 1
             WHERE id = $1 AND used = FALSE AND attempts < $2",
        )
        .bind(id)
        .bind(max_attempts)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(res.rows_affected() == 1)
    }

    async fn invalidate_email(&self, email: &str) -> Result<(), CoreError> {
        sqlx::query("UPDATE password_resets SET used = TRUE WHERE email = $1 AND used = FALSE")
            .bind(email)
            .execute(&self.pool)
            .await
            .map_err(map_db)?;
        Ok(())
    }
}
