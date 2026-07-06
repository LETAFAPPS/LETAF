use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::BaseFields;

/// Entidade Subcategoria — agrupamento de produtos dentro de uma Categoria.
///
/// Regras aplicadas (AI_RULES.md §6, §11):
/// - Campos base obrigatórios (UUID, company_id, timestamps, synced).
/// - Atrelada a uma Categoria via `category_id` (FK no banco).
/// - Isolamento multi-tenant garantido pelo `company_id` (validado no service).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subcategory {
    #[serde(flatten)]
    pub base: BaseFields,
    pub category_id: Uuid,
    pub name: String,
    #[serde(default)]
    pub sort_order: i32,
}

impl Subcategory {
    pub fn new(company_id: Uuid, category_id: Uuid, name: String) -> Self {
        Self {
            base: BaseFields::new(company_id),
            category_id,
            name,
            sort_order: 0,
        }
    }
}
