use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Tipo de empresa (ramo do estabelecimento) — catálogo de PLATAFORMA gerido
/// pelo super admin (global, sem `company_id`; exceção documentada ao
/// multi-tenant, como os planos). Ex.: "Restaurante", "Loja". Cada empresa
/// terá um tipo (associação em fase futura); por tipo virão tema do site e
/// diferenças de produto — ainda a implementar.
///
/// Regras (AI_RULES.md §6/§10): id UUID, soft delete (`deleted_at`), acesso a
/// dados só via repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusinessType {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub active: bool,
    pub sort_order: i32,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    #[serde(default)]
    pub deleted_at: Option<NaiveDateTime>,
}
