use async_trait::async_trait;
use chrono::NaiveDateTime;
use uuid::Uuid;

use super::model::Addon;
use crate::error::CoreError;

/// Trait de acesso a dados para Addon.
///
/// Regras aplicadas (AI_RULES.md §10, §11):
/// - Acesso ao banco somente via repository.
/// - Todas as operações filtram por `company_id` (isolamento multi-tenant).
#[async_trait]
pub trait AddonRepository: Send + Sync {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Addon>, CoreError>;
    async fn find_all(&self, company_id: Uuid) -> Result<Vec<Addon>, CoreError>;
    /// Lista todos os addons de um grupo (ordem `sort_order`). Inclui
    /// inativos — o consumidor (catálogo público) deve filtrar.
    async fn find_by_group(&self, company_id: Uuid, group_id: Uuid) -> Result<Vec<Addon>, CoreError>;
    async fn create(&self, addon: &Addon) -> Result<(), CoreError>;
    async fn update(&self, addon: &Addon) -> Result<(), CoreError>;
    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError>;
    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Addon>, CoreError>;
    async fn mark_synced(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError>;
    async fn sync_upsert(&self, addon: &Addon) -> Result<(), CoreError>;
    async fn find_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<Addon>, CoreError>;
}
