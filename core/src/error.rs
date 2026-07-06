use thiserror::Error;

/// Erros do domínio (core).
///
/// Regras aplicadas (AI_RULES.md §8):
/// - Código modular e legível
/// - Tipos de erro claros e descritivos
#[derive(Debug, Error, PartialEq)]
pub enum CoreError {
    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Validation: {0}")]
    Validation(String),

    #[error("Repository: {0}")]
    Repository(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),
}
