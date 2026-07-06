use async_trait::async_trait;
use uuid::Uuid;

use crate::error::CoreError;

use super::model::PasswordReset;

/// Acesso a dados dos pedidos de redefinição de senha (§10 — só via
/// repository). Implementado no servidor (PostgreSQL).
#[async_trait]
pub trait PasswordResetRepository: Send + Sync {
    /// Persiste um novo código.
    async fn create(&self, reset: &PasswordReset) -> Result<(), CoreError>;

    /// Código ativo (não usado) mais recente de um e-mail, se houver.
    async fn find_active(&self, email: &str) -> Result<Option<PasswordReset>, CoreError>;

    /// Marca um código como usado (consumido).
    async fn mark_used(&self, id: Uuid) -> Result<(), CoreError>;

    /// Invalida todos os códigos ativos de um e-mail (ao emitir um novo,
    /// evita vários códigos válidos ao mesmo tempo).
    async fn invalidate_email(&self, email: &str) -> Result<(), CoreError>;
}
