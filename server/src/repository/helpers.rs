use sqlx::PgPool;

use letaf_core::error::CoreError;

/// Converte erro sqlx em CoreError::Repository.
///
/// Regras aplicadas (AI_RULES.md §8, §10):
/// - Evitar duplicação de código
/// - Acesso ao banco somente via repository
pub fn map_db(e: sqlx::Error) -> CoreError {
    CoreError::Repository(e.to_string())
}

/// Verifica conectividade com PostgreSQL.
///
/// Regras aplicadas (AI_RULES.md §5, §10):
/// - Nunca acessar banco fora da camada repository
pub async fn check_db(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT 1").execute(pool).await?;
    Ok(())
}
