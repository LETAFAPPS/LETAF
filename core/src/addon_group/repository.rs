use async_trait::async_trait;
use chrono::NaiveDateTime;
use uuid::Uuid;

use super::model::AddonGroup;
use crate::error::CoreError;

/// Trait de acesso a dados para AddonGroup.
///
/// Regras aplicadas (AI_RULES.md §10, §11):
/// - Acesso ao banco somente via repository.
/// - Todas as operações filtram por `company_id` (isolamento multi-tenant).
#[async_trait]
pub trait AddonGroupRepository: Send + Sync {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<AddonGroup>, CoreError>;
    async fn find_all(&self, company_id: Uuid) -> Result<Vec<AddonGroup>, CoreError>;
    /// Lista os grupos associados a um produto específico (via tabela
    /// de junção `product_addon_groups`). Retorna em ordem `sort_order`.
    async fn find_by_product(&self, company_id: Uuid, product_id: Uuid) -> Result<Vec<AddonGroup>, CoreError>;
    async fn create(&self, group: &AddonGroup) -> Result<(), CoreError>;
    async fn update(&self, group: &AddonGroup) -> Result<(), CoreError>;
    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError>;
    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<AddonGroup>, CoreError>;
    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError>;
    /// Upsert de sincronização (§7.7 — last-write-wins via updated_at).
    async fn sync_upsert(&self, group: &AddonGroup) -> Result<(), CoreError>;
    /// Busca entidades atualizadas após o timestamp (§7 — sync pull).
    async fn find_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<AddonGroup>, CoreError>;
}
