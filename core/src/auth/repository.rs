use async_trait::async_trait;
use chrono::NaiveDateTime;
use uuid::Uuid;

use super::model::User;
use crate::error::CoreError;

/// Trait de acesso a dados para User.
///
/// Regras aplicadas (AI_RULES.md §10):
/// - Acesso ao banco somente via repository
/// - Usar traits para abstração
///
/// Todas as queries filtram por company_id (§3 — isolamento).
#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<User>, CoreError>;
    async fn find_by_email(&self, company_id: Uuid, email: &str) -> Result<Option<User>, CoreError>;
    /// Busca por e-mail INCLUINDO registros soft-deleted (deleted_at != NULL).
    /// Usado ao criar funcionário para reaproveitar um e-mail que pertencia
    /// a um funcionário excluído (a UNIQUE (company_id, email) é total).
    async fn find_by_email_any(&self, company_id: Uuid, email: &str) -> Result<Option<User>, CoreError>;
    async fn find_all(&self, company_id: Uuid) -> Result<Vec<User>, CoreError>;
    async fn create(&self, user: &User) -> Result<(), CoreError>;
    async fn update(&self, user: &User) -> Result<(), CoreError>;
    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError>;
    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<User>, CoreError>;
    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError>;

    /// Upsert de sincronização (§7.7 — last-write-wins via updated_at).
    async fn sync_upsert(&self, user: &User) -> Result<(), CoreError>;

    /// Busca entidades atualizadas após o timestamp (§7 — sync pull).
    async fn find_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<User>, CoreError>;

    /// Busca um usuário por email sem filtro de company_id.
    ///
    /// Usado exclusivamente no login desktop para identificar a empresa
    /// automaticamente a partir do email. Exceção documentada ao §11
    /// (isolamento por company_id): necessário para resolver o tenant
    /// antes da autenticação.
    async fn find_by_email_global(&self, email: &str) -> Result<Option<User>, CoreError>;

    /// Versão de credencial atual do usuário (RBAC §11 — revogação de JWT).
    /// `None` se o usuário não existe ou está soft-deletado (serve também de
    /// checagem de existência). Default (desktop, que não valida JWT): `Some(0)`
    /// se o usuário existe — o servidor (Postgres) sobrescreve com o valor real.
    async fn find_token_version(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<Option<i32>, CoreError> {
        Ok(self.find_by_id(company_id, id).await?.map(|_| 0))
    }

    /// Incrementa a versão de credencial (invalida tokens emitidos antes).
    /// Chamado ao mudar role/permissões ou senha. Default: no-op (só o
    /// servidor versiona).
    async fn bump_token_version(&self, _company_id: Uuid, _id: Uuid) -> Result<(), CoreError> {
        Ok(())
    }

    /// Incrementa a versão de credencial de TODOS os usuários de uma função
    /// (job_role). Chamado quando as permissões da função mudam — revoga os
    /// tokens de quem a possui. Default: no-op (só o servidor versiona).
    async fn bump_token_version_by_job_role(
        &self,
        _company_id: Uuid,
        _job_role_id: Uuid,
    ) -> Result<(), CoreError> {
        Ok(())
    }
}
