use chrono::{NaiveDate, NaiveDateTime};
use sqlx::{Sqlite, Transaction};
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

/// Formata NaiveDateTime como TEXT para SQLite.
pub fn ts(dt: NaiveDateTime) -> String {
    dt.format("%Y-%m-%d %H:%M:%S%.6f").to_string()
}

/// Parseia TEXT do SQLite para NaiveDateTime.
pub fn parse_timestamp(s: &str) -> Result<NaiveDateTime, CoreError> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f"))
        .map_err(|e| CoreError::Repository(format!("Invalid timestamp: {e}")))
}

/// Parseia TEXT do SQLite para Uuid.
pub fn parse_uuid(s: &str) -> Result<Uuid, CoreError> {
    Uuid::parse_str(s).map_err(|e| CoreError::Repository(format!("Invalid UUID: {e}")))
}

/// Grava um movimento de estoque (ledger append-only) DENTRO de uma transação
/// já aberta — usado por toda operação que altera `stock_quantity` para manter
/// o delta e o valor materializado atômicos (AI_RULES §6, §7). `now` é o
/// timestamp já formatado (`ts`), compartilhado com o UPDATE do produto.
#[allow(clippy::too_many_arguments)]
pub async fn insert_stock_movement(
    tx: &mut Transaction<'_, Sqlite>,
    company_id: Uuid,
    product_id: Uuid,
    delta: f64,
    reason: &str,
    order_id: Option<Uuid>,
    now: &str,
) -> Result<(), CoreError> {
    sqlx::query(
        "INSERT INTO stock_movements
            (id, company_id, product_id, delta, reason, order_id, created_at, updated_at, deleted_at, synced)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, NULL, 0)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(company_id.to_string())
    .bind(product_id.to_string())
    .bind(delta)
    .bind(reason)
    .bind(order_id.map(|o| o.to_string()))
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(map_db)?;
    Ok(())
}

/// Formata NaiveDate como TEXT (ISO-8601 `YYYY-MM-DD`).
pub fn date_str(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

/// Parseia TEXT do SQLite para NaiveDate.
pub fn parse_date(s: &str) -> Result<NaiveDate, CoreError> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|e| CoreError::Repository(format!("Invalid date: {e}")))
}
