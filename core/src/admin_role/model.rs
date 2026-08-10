use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Catálogo de TELAS do painel do super admin que uma Função pode liberar.
/// `(chave, rótulo_pt_br)`. Fonte única para UI (checkboxes), validação
/// (§11) e o "todas" do super admin master.
pub const SCREENS: &[(&str, &str)] = &[
    ("overview", "Dashboard"),
    ("companies", "Empresas"),
    ("plans", "Planos"),
    ("admins", "Usuários"),
    ("roles", "Funções"),
];

/// `true` se `key` é uma tela válida do catálogo (§11 — valida entrada).
pub fn screen_is_valid(key: &str) -> bool {
    SCREENS.iter().any(|(k, _)| *k == key)
}

/// Função de administrador (nível PLATAFORMA, global — exceção documentada
/// ao multi-tenant, igual ao super admin/planos). Define quais TELAS do
/// painel um usuário com esta função acessa.
///
/// Regras (AI_RULES §6/§10): id UUID, soft delete, acesso só via repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminRole {
    pub id: Uuid,
    pub name: String,
    /// Telas liberadas (chaves do catálogo `SCREENS`).
    #[serde(default)]
    pub screens: Vec<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    #[serde(default)]
    pub deleted_at: Option<NaiveDateTime>,
}
