use async_trait::async_trait;

use super::model::PaymentCharge;
use crate::error::CoreError;

/// Resultado da criação de cobrança remota — preenche os campos
/// que o core não consegue gerar (todos provenientes do gateway).
#[derive(Debug, Clone)]
pub struct CreatedCharge {
    pub txid: String,
    pub pix_copia_cola: String,
    pub qr_code_b64: String,
    pub expires_at: Option<chrono::NaiveDateTime>,
}

/// Status atual da cobrança no gateway. Apenas os campos que mudam
/// no ciclo de vida — `paid_at` é populado quando confirmado.
#[derive(Debug, Clone)]
pub struct ChargeStatusUpdate {
    pub status: super::model::ChargeStatus,
    pub paid_at: Option<chrono::NaiveDateTime>,
    pub last_error: Option<String>,
}

/// Trait abstrata do gateway. Mantém o core agnóstico de Efi/Pagar.me/etc.
///
/// Regras aplicadas (AI_RULES.md §1, §11):
/// - Toda chamada de rede vive na implementação concreta (server).
/// - Inputs/outputs em tipos do domínio — sem JSON cru.
#[async_trait]
pub trait PaymentGateway: Send + Sync {
    /// Cria uma cobrança PIX no gateway. `charge` já existe localmente
    /// (status `Pending`, sem `txid`) — o gateway preenche o restante.
    async fn create_pix_charge(
        &self,
        charge: &PaymentCharge,
        description: &str,
    ) -> Result<CreatedCharge, CoreError>;

    /// Consulta status atual no gateway. Usado pelo polling enquanto
    /// não tem webhook ativo. `txid` é o identificador remoto.
    async fn fetch_charge_status(&self, txid: &str)
        -> Result<ChargeStatusUpdate, CoreError>;

    /// Nome do gateway (para coluna `gateway` em payment_charges).
    fn name(&self) -> &str;
}
