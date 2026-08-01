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

    /// Cobranças ainda pendentes (com `txid`) criadas depois de `created_after`,
    /// de TODAS as empresas — insumo do reconciliador que roda no servidor.
    ///
    /// Sem filtro por tenant de propósito: o laço é do processo, não de uma
    /// requisição, e cada cobrança carrega o próprio `company_id`. O corte por
    /// data evita arrastar cobranças velhas que o gateway já expirou.
    async fn find_pending(
        &self,
        created_after: chrono::NaiveDateTime,
        limit: i64,
    ) -> Result<Vec<PaymentCharge>, CoreError>;
}
