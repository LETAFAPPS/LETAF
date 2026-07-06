use async_trait::async_trait;
use chrono::NaiveDateTime;
use uuid::Uuid;

use super::model::JobRole;
use crate::error::CoreError;

/// Trait de acesso a dados para JobRole (função/cargo).
///
/// Regras (AI_RULES.md §10): acesso ao banco somente via repository;
/// traits para abstração.
#[async_trait]
pub trait JobRoleRepository: Send + Sync {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<JobRole>, CoreError>;
    async fn find_all(&self, company_id: Uuid) -> Result<Vec<JobRole>, CoreError>;
    async fn create(&self, role: &JobRole) -> Result<(), CoreError>;
    async fn update(&self, role: &JobRole) -> Result<(), CoreError>;
    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError>;
    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<JobRole>, CoreError>;
    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError>;

    /// Upsert de sincronização (§7.7 — last-write-wins via updated_at).
    async fn sync_upsert(&self, role: &JobRole) -> Result<(), CoreError>;

    /// Busca funções atualizadas após o timestamp (§7 — sync pull).
    async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
    ) -> Result<Vec<JobRole>, CoreError>;
}
