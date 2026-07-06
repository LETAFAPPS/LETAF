use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::BaseFields;

/// Forma de pagamento cadastrada (1 linha por método).
///
/// Regras aplicadas (AI_RULES.md §6, §11):
/// - Sem CVV/número completo do cartão (este modelo é só catálogo).
/// - `is_default = true` em exatamente 1 método por company.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentMethod {
    #[serde(flatten)]
    pub base: BaseFields,
    /// "card" | "pix".
    pub kind: String,
    /// "Visa de crédito" | "PIX automático" | "Mastercard"...
    pub label: String,
    /// "••••4242" (cartão) ou "" (PIX).
    pub masked: String,
    /// "08/28" (cartão) ou "" (PIX).
    pub expiry: String,
    pub is_default: bool,
}

impl PaymentMethod {
    pub fn new_card(company_id: Uuid, label: String, masked: String, expiry: String) -> Self {
        Self {
            base: BaseFields::new(company_id),
            kind: "card".into(),
            label,
            masked,
            expiry,
            is_default: false,
        }
    }

    pub fn new_pix(company_id: Uuid, label: String) -> Self {
        Self {
            base: BaseFields::new(company_id),
            kind: "pix".into(),
            label,
            masked: String::new(),
            expiry: String::new(),
            is_default: false,
        }
    }
}
