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
}
