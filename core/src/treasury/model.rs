use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::BaseFields;

/// Carteira do estabelecimento (tesouraria).
///
/// Regras aplicadas (AI_RULES.md §6, §11):
/// - SINGLETON por empresa: unique em `company_id` na migration; o
///   service rejeita a criação de uma segunda carteira.
/// - `initial_balance` é o saldo inicial declarado pelo operador ao
///   abrir a carteira (`>= 0`, validado no service).
/// - `notes` é observação livre do operador (opcional).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Treasury {
    #[serde(flatten)]
    pub base: BaseFields,
    /// Saldo inicial do estabelecimento.
    pub initial_balance: Decimal,
    #[serde(default)]
    pub notes: Option<String>,
}

impl Treasury {
    pub fn new(company_id: Uuid, initial_balance: Decimal, notes: Option<String>) -> Self {
        Self {
            base: BaseFields::new(company_id),
            initial_balance,
            notes,
        }
    }
}
