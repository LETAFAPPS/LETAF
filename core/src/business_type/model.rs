use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Temas visuais disponíveis para o site (cardápio) por tipo de empresa.
/// `(slug, rótulo_pt_br)`. Fonte única: validação no service, opções na UI do
/// painel e presets de CSS no `web` (`:root[data-theme="<slug>"]`). O slug
/// `restaurante` é o visual atual do sistema (default).
pub const THEMES: &[(&str, &str)] = &[
    ("restaurante", "Restaurante"),
    ("loja", "Loja"),
    ("fabrica", "Fábrica"),
];

/// Slug do tema default (visual atual do sistema).
pub const DEFAULT_THEME: &str = "restaurante";

/// `true` se `slug` é um tema válido do catálogo `THEMES`.
pub fn theme_is_valid(slug: &str) -> bool {
    THEMES.iter().any(|(s, _)| *s == slug)
}

fn default_theme() -> String {
    DEFAULT_THEME.to_string()
}

/// Tipo de empresa (ramo do estabelecimento) — catálogo de PLATAFORMA gerido
/// pelo super admin (global, sem `company_id`; exceção documentada ao
/// multi-tenant, como os planos). Ex.: "Restaurante", "Loja". Cada empresa
/// tem um tipo (ver `Company::business_type_id`); o `theme` define o visual do
/// site (fase B). Diferenças de produto por tipo virão na fase C.
///
/// Regras (AI_RULES.md §6/§10): id UUID, soft delete (`deleted_at`), acesso a
/// dados só via repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusinessType {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Tema visual do site para este tipo (slug de `THEMES`). Default
    /// `restaurante` (visual atual). Validado no service.
    #[serde(default = "default_theme")]
    pub theme: String,
    pub active: bool,
    pub sort_order: i32,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    #[serde(default)]
    pub deleted_at: Option<NaiveDateTime>,
}
