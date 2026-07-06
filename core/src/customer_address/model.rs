use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::BaseFields;

/// Endereço de entrega do cliente.
///
/// Regras aplicadas (AI_RULES.md §6):
/// - Campos base obrigatórios (UUID, company_id, timestamps, synced)
/// - Vinculado ao cliente por customer_id
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomerAddress {
    #[serde(flatten)]
    pub base: BaseFields,
    pub customer_id: Uuid,
    /// Tipo: "Casa", "Trabalho" ou "Outros"
    pub label: String,
    /// Nome personalizado quando label = "Outros"
    pub custom_label: Option<String>,
    pub street: String,
    pub number: String,
    pub neighborhood: String,
    pub apartment: Option<String>,
}

impl CustomerAddress {
    pub fn new(
        company_id: Uuid,
        customer_id: Uuid,
        label: String,
        custom_label: Option<String>,
        street: String,
        number: String,
        neighborhood: String,
        apartment: Option<String>,
    ) -> Self {
        Self {
            base: BaseFields::new(company_id),
            customer_id,
            label,
            custom_label,
            street,
            number,
            neighborhood,
            apartment,
        }
    }

    /// Rótulo de exibição: custom_label (Outros) ou label.
    pub fn display_label(&self) -> &str {
        self.custom_label.as_deref().unwrap_or(&self.label)
    }
}
