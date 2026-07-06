use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::BaseFields;

/// Função (cargo) com um conjunto de permissões granulares.
///
/// Regras (AI_RULES.md §6, §11): id UUID, company_id, soft delete e
/// `synced` via `BaseFields`. Atribuída a Funcionários (`Employee`) para
/// restringir o acesso; o `Admin` tem acesso total e não depende de
/// função. As permissões são chaves do catálogo [`crate::permission`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRole {
    #[serde(flatten)]
    pub base: BaseFields,
    pub name: String,
    /// Chaves `"feature.action"` concedidas (ver [`crate::permission`]).
    #[serde(default)]
    pub permissions: Vec<String>,
}

impl JobRole {
    pub fn new(company_id: Uuid, name: String, permissions: Vec<String>) -> Self {
        Self {
            base: BaseFields::new(company_id),
            name,
            permissions,
        }
    }

    /// `true` se esta função concede a permissão `perm`.
    pub fn has(&self, perm: &str) -> bool {
        self.permissions.iter().any(|p| p == perm)
    }
}
