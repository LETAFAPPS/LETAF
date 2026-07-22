use async_trait::async_trait;
use uuid::Uuid;

use super::model::PaymentCharge;
use crate::error::CoreError;

/// Persistência das cobranças. Sync com server é desejável para
/// auditoria, mas a primeira versão só persiste local — o estado
/// canônico vive no gateway.
#[async_trait]
pub trait PaymentChargeRepository: Send + Sync {
    async fn find_by_id(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<Option<PaymentCharge>, CoreError>;

    async fn create(&self, charge: &PaymentCharge) -> Result<(), CoreError>;
    async fn update(&self, charge: &PaymentCharge) -> Result<(), CoreError>;
}
