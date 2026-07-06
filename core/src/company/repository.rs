use async_trait::async_trait;
use chrono::NaiveDateTime;
use uuid::Uuid;

use super::model::Company;
use crate::error::CoreError;

/// Trait de acesso a dados para Company.
///
/// Regras aplicadas (AI_RULES.md §10):
/// - Acesso ao banco somente via repository
/// - Usar traits para abstração
#[async_trait]
pub trait CompanyRepository: Send + Sync {
    async fn find_by_id(&self, id: Uuid) -> Result<Option<Company>, CoreError>;
    async fn find_by_subdomain(&self, subdomain: &str) -> Result<Option<Company>, CoreError>;
    async fn find_all(&self) -> Result<Vec<Company>, CoreError>;
    async fn create(&self, company: &Company) -> Result<(), CoreError>;
    async fn update(&self, company: &Company) -> Result<(), CoreError>;
    async fn soft_delete(&self, id: Uuid) -> Result<(), CoreError>;
    async fn find_unsynced(&self) -> Result<Vec<Company>, CoreError>;
    async fn mark_synced(&self, id: Uuid) -> Result<(), CoreError>;

    /// Upsert de sincronização (§7.7 — last-write-wins via updated_at).
    async fn sync_upsert(&self, company: &Company) -> Result<(), CoreError>;

    /// Busca empresa atualizada após o timestamp (§7 — sync pull).
    async fn find_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<Company>, CoreError>;
}
