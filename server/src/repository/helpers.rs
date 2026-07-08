use chrono::NaiveDateTime;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use letaf_core::error::CoreError;

/// Converte erro sqlx em CoreError::Repository.
///
/// Regras aplicadas (AI_RULES.md §8, §10):
/// - Evitar duplicação de código
/// - Acesso ao banco somente via repository
pub fn map_db(e: sqlx::Error) -> CoreError {
    CoreError::Repository(e.to_string())
}

/// Grava um movimento de estoque (ledger append-only) DENTRO de uma transação
/// já aberta — toda operação do servidor que altera `stock_quantity` (pedidos
/// web, ajustes) registra o delta atomicamente para propagar aos desktops via
/// pull idempotente, sem LWW sobre o absoluto (AI_RULES §6, §7). `synced=true`:
/// o servidor é o hub (não empurra); o desktop puxa por `updated_at`.
#[allow(clippy::too_many_arguments)]
pub async fn insert_stock_movement(
    tx: &mut Transaction<'_, Postgres>,
    company_id: Uuid,
    product_id: Uuid,
    delta: f64,
    reason: &str,
    order_id: Option<Uuid>,
    now: NaiveDateTime,
) -> Result<(), CoreError> {
    sqlx::query(
        "INSERT INTO stock_movements
            (id, company_id, product_id, delta, reason, order_id, created_at, updated_at, deleted_at, synced)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $7, NULL, true)",
    )
    .bind(Uuid::new_v4())
    .bind(company_id)
    .bind(product_id)
    .bind(delta)
    .bind(reason)
    .bind(order_id)
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(map_db)?;
    Ok(())
}

/// Monta o SELECT paginado por keyset `(updated_at, id)` do pull, para uma
/// tabela sincronizável. Fonte única do SQL repetido nos
/// `find_updated_since_paged` das entidades grandes (§8/§14 — sem duplicação).
/// `table` é sempre um literal interno (nunca entrada do cliente), então a
/// interpolação é segura.
///
/// Binds esperados, nesta ordem: `$1 company_id`, `$2 since`, `$3 after_id`,
/// `$4 limit`.
pub fn keyset_pull_sql(table: &str) -> String {
    format!(
        "SELECT * FROM {table}
          WHERE company_id = $1
            AND (updated_at > $2 OR (updated_at = $2 AND id > $3))
          ORDER BY updated_at ASC, id ASC
          LIMIT $4"
    )
}

/// Verifica conectividade com PostgreSQL.
///
/// Regras aplicadas (AI_RULES.md §5, §10):
/// - Nunca acessar banco fora da camada repository
pub async fn check_db(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT 1").execute(pool).await?;
    Ok(())
}
