use chrono::{NaiveDate, NaiveDateTime};
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

/// Formata NaiveDate como TEXT (ISO-8601 `YYYY-MM-DD`).
pub fn date_str(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

/// Parseia TEXT do SQLite para NaiveDate.
pub fn parse_date(s: &str) -> Result<NaiveDate, CoreError> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|e| CoreError::Repository(format!("Invalid date: {e}")))
}
