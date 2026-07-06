use async_trait::async_trait;
use chrono::NaiveDateTime;
use uuid::Uuid;

use super::model::Subcategory;
use crate::error::CoreError;

/// Trait de acesso a dados para Subcategory.
///
/// Regras aplicadas (AI_RULES.md §10, §11):
/// - Acesso ao banco somente via repository.
/// - Todas as operações filtram por `company_id` (isolamento multi-tenant).
#[async_trait]
pub trait SubcategoryRepository: Send + Sync {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Subcategory>, CoreError>;
    async fn find_all(&self, company_id: Uuid) -> Result<Vec<Subcategory>, CoreError>;
    /// Lista subcategorias de uma categoria específica.
    async fn find_by_category(&self, company_id: Uuid, category_id: Uuid) -> Result<Vec<Subcategory>, CoreError>;
    async fn create(&self, subcategory: &Subcategory) -> Result<(), CoreError>;
    async fn update(&self, subcategory: &Subcategory) -> Result<(), CoreError>;
    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError>;
    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Subcategory>, CoreError>;
    async fn mark_synced(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError>;

    /// Upsert de sincronização (§7.7 — last-write-wins via updated_at).
    async fn sync_upsert(&self, subcategory: &Subcategory) -> Result<(), CoreError>;

    /// Busca entidades atualizadas após o timestamp (§7 — sync pull).
    async fn find_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<Subcategory>, CoreError>;
}
