use serde::{Deserialize, Serialize};

use crate::entity::BaseFields;

/// Entidade Categoria — agrupa produtos por tipo.
///
/// Regras aplicadas (AI_RULES.md §6):
/// - Campos base obrigatórios (UUID, company_id, timestamps, synced)
/// - Campos de domínio: name, description
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    #[serde(flatten)]
    pub base: BaseFields,
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub sort_order: i32,
    /// Slug do ícone (allowlist em `category::icons::ICONS`). `None`
    /// = sem ícone (UI renderiza placeholder neutro). Slugs antigos
    /// que foram removidos da allowlist também caem nesse caminho —
    /// nada quebra.
    #[serde(default)]
    pub icon_name: Option<String>,
}

impl Category {
    pub fn new(
        company_id: uuid::Uuid,
        name: String,
        description: Option<String>,
    ) -> Self {
        Self {
            base: BaseFields::new(company_id),
            name,
            description,
            sort_order: 0,
            icon_name: None,
        }
    }
}
