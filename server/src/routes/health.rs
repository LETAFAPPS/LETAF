use axum::extract::State;
use axum::{routing::get, Json, Router};
use serde_json::{json, Value};

use crate::context::AppState;
use crate::error::ServerError;
use crate::repository::helpers::check_db;

pub fn routes() -> Router<AppState> {
    Router::new().route("/health", get(health_check))
}

/// Health check que verifica conectividade com PostgreSQL.
///
/// Regras aplicadas (AI_RULES.md §4, §5, §10, §12):
/// - Backend usa SQLx
/// - Acesso ao banco somente via camada repository
/// - Respostas sempre em JSON
async fn health_check(
    State(state): State<AppState>,
) -> Result<Json<Value>, ServerError> {
    check_db(&state.pool).await?;

    Ok(Json(json!({ "status": "ok" })))
}
