use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use thiserror::Error;

use letaf_core::error::CoreError;

/// Erros da camada server.
///
/// Converte CoreError e erros de infraestrutura em respostas HTTP JSON.
///
/// Regras aplicadas (AI_RULES.md §12):
/// - Respostas sempre em JSON
#[derive(Debug, Error)]
pub enum ServerError {
    #[error("{0}")]
    Core(#[from] CoreError),

    #[error("Database: {0}")]
    Database(#[from] sqlx::Error),

    #[error("JWT: {0}")]
    Jwt(String),

    /// Autenticado mas sem permissão para a operação (RBAC §11). 403,
    /// distinto do 401 (token inválido) para não confundir clientes que
    /// reagem a 401 com re-login.
    #[error("{0}")]
    Forbidden(String),

    #[error("Tenant not identified")]
    TenantNotFound,

    /// Recurso indisponível por config ausente (ex: gateway de pagamento
    /// não configurado). 503 com mensagem explícita ao invés de 500.
    #[error("{0}")]
    ServiceUnavailable(&'static str),

    /// Excesso de requisições (rate limit) — 429. Usado nos endpoints de
    /// autenticação para frear brute force (§11).
    #[error("{0}")]
    TooManyRequests(&'static str),
}

impl IntoResponse for ServerError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            ServerError::Core(CoreError::NotFound(msg)) => {
                (StatusCode::NOT_FOUND, msg.clone())
            }
            ServerError::Core(CoreError::Validation(msg)) => {
                (StatusCode::BAD_REQUEST, msg.clone())
            }
            ServerError::Core(CoreError::Unauthorized(msg)) => {
                (StatusCode::UNAUTHORIZED, msg.clone())
            }
            ServerError::Core(CoreError::Repository(msg)) => {
                tracing::error!("Repository error: {msg}");
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error".into())
            }
            ServerError::Database(e) => {
                tracing::error!("Database error: {e}");
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error".into())
            }
            ServerError::Jwt(msg) => {
                (StatusCode::UNAUTHORIZED, msg.clone())
            }
            ServerError::Forbidden(msg) => {
                (StatusCode::FORBIDDEN, msg.clone())
            }
            ServerError::TenantNotFound => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            ServerError::ServiceUnavailable(msg) => {
                (StatusCode::SERVICE_UNAVAILABLE, (*msg).to_string())
            }
            ServerError::TooManyRequests(msg) => {
                (StatusCode::TOO_MANY_REQUESTS, (*msg).to_string())
            }
        };

        let body = Json(json!({ "error": message }));
        (status, body).into_response()
    }
}
