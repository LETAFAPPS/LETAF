use std::sync::Arc;

use chrono::{Duration, Utc};
use uuid::Uuid;

use crate::error::CoreError;

use super::model::PasswordReset;
use super::repository::PasswordResetRepository;

/// Validade do código de redefinição.
const CODE_TTL_MINUTES: i64 = 15;

/// Regras de redefinição de senha (RBAC/§11): gera e valida o código de
/// uso único. NÃO envia e-mail nem altera a senha — isso é orquestrado no
/// servidor (o envio é infra; a troca da senha é do `AuthService`).
pub struct PasswordResetService {
    repo: Arc<dyn PasswordResetRepository>,
}

impl PasswordResetService {
    pub fn new(repo: Arc<dyn PasswordResetRepository>) -> Self {
        Self { repo }
    }

    /// Emite um código de 6 dígitos: invalida os anteriores do e-mail,
    /// persiste o HASH do novo e devolve o código EM CLARO (para o caller
    /// enviar por e-mail). Requer feature `password-hashing` (bcrypt).
    #[cfg(feature = "password-hashing")]
    pub async fn issue_code(&self, email: &str) -> Result<String, CoreError> {
        let code = super::gen_code();
        let code_hash = crate::hashing::hash_password(code.clone()).await?;
        self.repo.invalidate_email(email).await?;
        let now = Utc::now().naive_utc();
        let reset = PasswordReset {
            id: Uuid::new_v4(),
            email: email.to_string(),
            code_hash,
            expires_at: now + Duration::minutes(CODE_TTL_MINUTES),
            used: false,
            created_at: now,
        };
        self.repo.create(&reset).await?;
        Ok(code)
    }

    /// Localiza o código ATIVO do e-mail e valida expiração + correspondência,
    /// SEM consumir. Mensagem genérica para não vazar detalhes (§11).
    #[cfg(feature = "password-hashing")]
    async fn find_valid(&self, email: &str, code: &str) -> Result<PasswordReset, CoreError> {
        let invalid = || CoreError::Validation("Código inválido ou expirado".into());
        let reset = self.repo.find_active(email).await?.ok_or_else(invalid)?;
        if reset.expires_at < Utc::now().naive_utc() {
            return Err(invalid());
        }
        let ok = crate::hashing::verify_password(code.to_string(), reset.code_hash.clone()).await?;
        if !ok {
            return Err(invalid());
        }
        Ok(reset)
    }

    /// Valida o código SEM consumir — usado para confirmar o código antes
    /// de o usuário digitar a nova senha (a troca final revalida e consome).
    #[cfg(feature = "password-hashing")]
    pub async fn verify_code(&self, email: &str, code: &str) -> Result<(), CoreError> {
        self.find_valid(email, code).await.map(|_| ())
    }

    /// Valida o código (ativo, não expirado e correspondente) e o consome
    /// (marca como usado). Mensagem genérica para não vazar detalhes (§11).
    #[cfg(feature = "password-hashing")]
    pub async fn verify_and_consume(&self, email: &str, code: &str) -> Result<(), CoreError> {
        let reset = self.find_valid(email, code).await?;
        self.repo.mark_used(reset.id).await?;
        Ok(())
    }
}
