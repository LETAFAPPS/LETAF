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

    /// Credencial inválida/ausente → o cliente deve reautenticar (401).
    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    /// Autenticado, mas SEM DIREITO sobre aquele dado → 403.
    ///
    /// Distinção crítica para o sync: o worker trata 401 como "JWT
    /// expirado" e APAGA o token. Uma recusa de autorização devolvida
    /// como 401 derrubava a sessão a cada ciclo — o operador era
    /// deslogado, relogava, o registro pendente era reenviado e o
    /// logout se repetia, deixando o terminal inutilizável (§7.6).
    #[error("Forbidden: {0}")]
    Forbidden(String),
}
