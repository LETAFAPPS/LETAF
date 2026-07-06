use async_trait::async_trait;
use chrono::NaiveDateTime;
use uuid::Uuid;

use super::model::CustomerAddress;
use crate::error::CoreError;

/// Trait de acesso a dados para CustomerAddress.
///
/// Regras aplicadas (AI_RULES.md §10):
/// - Acesso ao banco somente via repository
/// - Traits para abstração entre camadas
#[async_trait]
pub trait CustomerAddressRepository: Send + Sync {
    async fn find_by_customer(
        &self,
        company_id: Uuid,
        customer_id: Uuid,
    ) -> Result<Vec<CustomerAddress>, CoreError>;

    /// Todos os endereços (não deletados) da empresa — evita N+1 ao
    /// listar endereços de vários clientes de uma vez.
    async fn find_by_company(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<CustomerAddress>, CoreError>;

    async fn find_by_id(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<Option<CustomerAddress>, CoreError>;

    async fn create(&self, address: &CustomerAddress) -> Result<(), CoreError>;

    async fn update(&self, address: &CustomerAddress) -> Result<(), CoreError>;

    /// Soft delete garantindo que o endereço pertence ao customer (§11).
    async fn soft_delete(
        &self,
        company_id: Uuid,
        id: Uuid,
        customer_id: Uuid,
    ) -> Result<(), CoreError>;

    // ── Sincronização offline-first (§7) ────────────────────────
    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<CustomerAddress>, CoreError>;
    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError>;
    async fn sync_upsert(&self, address: &CustomerAddress) -> Result<(), CoreError>;
    async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
    ) -> Result<Vec<CustomerAddress>, CoreError>;
}
