use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::BaseFields;

/// Status canônico interno. Mapeia `ATIVA`/`CONCLUIDA`/`REMOVIDA_PELO_PSP`
/// da Efi e seus equivalentes em outros gateways.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChargeStatus {
    Pending,
    Paid,
    Expired,
    Failed,
    Cancelled,
}

impl ChargeStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Paid => "paid",
            Self::Expired => "expired",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    // `from_str` infalível (default em valor desconhecido); não é o
    // `FromStr` da std, que retorna `Result` — silenciamos o lint.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "paid" => Self::Paid,
            "expired" => Self::Expired,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => Self::Pending,
        }
    }

    pub fn is_terminal(self) -> bool {
        !matches!(self, Self::Pending)
    }
}

/// Cobrança avulsa em um gateway externo. Persistida para auditoria
/// + permitir reabrir o QR Code se o operador fechar a UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentCharge {
    #[serde(flatten)]
    pub base: BaseFields,
    /// `subscription_invoices.id` quando a cobrança é para quitar uma
    /// fatura existente; `None` para cobranças avulsas.
    pub invoice_id: Option<Uuid>,
    /// "efi", "pagar.me", etc.
    pub gateway: String,
    /// "pix", "card".
    pub method: String,
    /// TXID retornado pelo gateway. `None` antes da chamada remota.
    pub txid: Option<String>,
    pub amount: f64,
    pub status: ChargeStatus,
    pub pix_copia_cola: Option<String>,
    pub qr_code_b64: Option<String>,
    pub expires_at: Option<NaiveDateTime>,
    pub paid_at: Option<NaiveDateTime>,
    pub last_error: Option<String>,
}

impl PaymentCharge {
    pub fn new_pix(company_id: Uuid, invoice_id: Option<Uuid>, amount: f64) -> Self {
        Self {
            base: BaseFields::new(company_id),
            invoice_id,
            gateway: "efi".into(),
            method: "pix".into(),
            txid: None,
            amount,
            status: ChargeStatus::Pending,
            pix_copia_cola: None,
            qr_code_b64: None,
            expires_at: None,
            paid_at: None,
            last_error: None,
        }
    }
}
