use async_trait::async_trait;
use uuid::Uuid;

use crate::error::CoreError;

use super::model::BusinessType;

/// Acesso a dados do catálogo de tipos de empresa (§10 — só via repository).
/// Implementado no servidor (PostgreSQL). Global (sem company_id).
#[async_trait]
pub trait BusinessTypeRepository: Send + Sync {
    /// Todos os tipos não removidos, ordenados por `sort_order` (gestão).
    async fn find_all(&self) -> Result<Vec<BusinessType>, CoreError>;
    /// Apenas os tipos ATIVOS.
    async fn find_active(&self) -> Result<Vec<BusinessType>, CoreError>;
    async fn find_by_id(&self, id: Uuid) -> Result<Option<BusinessType>, CoreError>;
    async fn create(&self, item: &BusinessType) -> Result<(), CoreError>;
    async fn update(&self, item: &BusinessType) -> Result<(), CoreError>;
    async fn soft_delete(&self, id: Uuid) -> Result<(), CoreError>;
}
