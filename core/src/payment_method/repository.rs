use async_trait::async_trait;
use chrono::NaiveDateTime;
use uuid::Uuid;

use super::model::PaymentMethod;
use crate::error::CoreError;

#[async_trait]
pub trait PaymentMethodRepository: Send + Sync {
    async fn find_by_id(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<Option<PaymentMethod>, CoreError>;
    async fn find_all(&self, company_id: Uuid) -> Result<Vec<PaymentMethod>, CoreError>;
    async fn find_default(&self, company_id: Uuid) -> Result<Option<PaymentMethod>, CoreError>;
    async fn create(&self, method: &PaymentMethod) -> Result<(), CoreError>;
    async fn update(&self, method: &PaymentMethod) -> Result<(), CoreError>;
    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError>;
    /// Limpa `is_default` em todos os métodos da company (chamado antes
    /// de marcar um novo como default).
    async fn clear_default(&self, company_id: Uuid) -> Result<(), CoreError>;

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<PaymentMethod>, CoreError>;
    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError>;
    async fn sync_upsert(&self, method: &PaymentMethod) -> Result<(), CoreError>;
    async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
    ) -> Result<Vec<PaymentMethod>, CoreError>;
}
