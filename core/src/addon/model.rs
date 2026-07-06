use serde::{Deserialize, Serialize};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::entity::BaseFields;

/// Entidade Adicional — item individual dentro de um
/// [`crate::addon_group::model::AddonGroup`] (ex.: "Catupiry",
/// "Cheddar", "Sem cebola"). Cada adicional tem preço opcional que
/// é somado ao preço do produto no carrinho web.
///
/// Regras aplicadas (AI_RULES.md §6, §11):
/// - Campos base obrigatórios (UUID, company_id, timestamps, synced).
/// - Isolamento multi-tenant via `company_id` (validado no service).
/// - `group_id` precisa pertencer à mesma empresa (validado no service).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Addon {
    #[serde(flatten)]
    pub base: BaseFields,
    pub group_id: Uuid,
    pub name: String,
    /// Acréscimo em R$ somado ao preço base do produto. `0.0` é válido
    /// (ex.: "Sem cebola" gratuito).
    pub price: Decimal,
    /// Ordem dentro do grupo (asc).
    #[serde(default)]
    pub sort_order: i32,
    /// Quando `false`, o addon não aparece no cardápio web (mas pode
    /// continuar no PDV em uso administrativo). Default `true`.
    #[serde(default = "default_true")]
    pub active: bool,
}

fn default_true() -> bool {
    true
}

impl Addon {
    pub fn new(company_id: Uuid, group_id: Uuid, name: String, price: Decimal) -> Self {
        Self {
            base: BaseFields::new(company_id),
            group_id,
            name,
            price,
            sort_order: 0,
            active: true,
        }
    }
}
