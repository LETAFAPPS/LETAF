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

    /// Consome um código ATOMICAMENTE (marca usado só se ainda não usado).
    /// Retorna `true` se ESTA chamada consumiu (venceu a corrida); `false` se
    /// já estava usado. Garante uso único mesmo sob requisições concorrentes
    /// com o mesmo código (§11 — sem check-then-act).
    async fn mark_used(&self, id: Uuid) -> Result<bool, CoreError>;

    /// Reivindica ATOMICAMENTE um "slot" de tentativa ANTES da verificação cara
    /// (bcrypt). Incrementa `attempts` só se o código ainda está ativo e abaixo
    /// do teto (`used = FALSE AND attempts < max_attempts`); retorna `true` se um
    /// slot foi consumido (o chamador PODE verificar o hash), `false` se o código
    /// já se esgotou/foi usado (rejeita SEM hashear). Anti-força-bruta do código
    /// de 6 dígitos (§11): o `UPDATE` toma lock de linha, então uma rajada
    /// concorrente serializa e só os primeiros `max_attempts` palpites chegam ao
    /// bcrypt — o enforcement não vaza por concorrência.
    async fn claim_verify_attempt(&self, id: Uuid, max_attempts: i32) -> Result<bool, CoreError>;

    /// Invalida todos os códigos ativos de um e-mail (ao emitir um novo,
    /// evita vários códigos válidos ao mesmo tempo).
    async fn invalidate_email(&self, email: &str) -> Result<(), CoreError>;
}
