use async_trait::async_trait;
use chrono::NaiveDateTime;
use uuid::Uuid;

use super::model::Banner;
use crate::error::CoreError;

/// Trait de acesso a dados para Banner.
///
/// Regras aplicadas (AI_RULES.md §10):
/// - Acesso ao banco somente via repository.
/// - Espelha o padrão de Category/Product (todas operações filtradas
///   por `company_id` para isolamento multi-tenant).
#[async_trait]
pub trait BannerRepository: Send + Sync {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Banner>, CoreError>;
    async fn find_all(&self, company_id: Uuid) -> Result<Vec<Banner>, CoreError>;
    /// Apenas banners ativos, ordenados por `sort_order ASC`.
    /// Usado pela rota pública `/catalog/banners`.
    async fn find_active(&self, company_id: Uuid) -> Result<Vec<Banner>, CoreError>;
    async fn create(&self, banner: &Banner) -> Result<(), CoreError>;
    async fn update(&self, banner: &Banner) -> Result<(), CoreError>;
    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError>;
    async fn set_active(&self, company_id: Uuid, id: Uuid, active: bool) -> Result<(), CoreError>;

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Banner>, CoreError>;
    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError>;
    async fn sync_upsert(&self, banner: &Banner) -> Result<(), CoreError>;
    async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
    ) -> Result<Vec<Banner>, CoreError>;
}
