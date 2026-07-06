use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::BaseFields;

/// Banner promocional exibido no topo do cardápio web.
///
/// Regras aplicadas (AI_RULES.md §6, §8, §11):
/// - Campos base obrigatórios (UUID, company_id, timestamps, synced).
/// - `item_type` define o alvo do clique: `"product"` (navega ao
///   produto via `item_id`) ou `"url"` (abre `item_url` em nova aba).
/// - `image_data` armazena a imagem em base64 (mesmo padrão do logo
///   e dos produtos — funciona offline via sync, sem CDN externo).
/// - `active` controla se o banner aparece no público: rota
///   `/catalog/banners` só devolve `active = true`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Banner {
    #[serde(flatten)]
    pub base: BaseFields,
    pub title: String,
    pub image_data: String,
    /// `"product"` | `"url"`. Validado no service contra a allowlist.
    pub item_type: String,
    /// Quando `item_type == "product"`. UUID do produto associado.
    #[serde(default)]
    pub item_id: Option<Uuid>,
    /// Quando `item_type == "url"`. URL externa (http(s)).
    #[serde(default)]
    pub item_url: Option<String>,
    #[serde(default = "default_active")]
    pub active: bool,
    #[serde(default)]
    pub sort_order: i32,
}

fn default_active() -> bool { true }

impl Banner {
    pub fn new(
        company_id: Uuid,
        title: String,
        image_data: String,
        item_type: String,
        item_id: Option<Uuid>,
        item_url: Option<String>,
    ) -> Self {
        Self {
            base: BaseFields::new(company_id),
            title,
            image_data,
            item_type,
            item_id,
            item_url,
            active: true,
            sort_order: 0,
        }
    }
}

/// Tipos válidos para `Banner.item_type`. Mudanças aqui exigem
/// migração de dados se algum tipo for removido.
pub const ITEM_TYPES: &[&str] = &["product", "url"];
