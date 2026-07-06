use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::BaseFields;

/// Entidade Grupo de Adicionais — agrupamento de opções complementares
/// que podem ser ligadas a um ou mais produtos (ex.: "Borda", "Acompa-
/// nhamentos"). O grupo concentra a regra de seleção e seus itens
/// individuais vivem em [`crate::addon::model::Addon`].
///
/// Regras aplicadas (AI_RULES.md §6, §11):
/// - Campos base obrigatórios (UUID, company_id, timestamps, synced).
/// - Isolamento multi-tenant via `company_id` (validado no service).
/// - Modelado como entidade independente (não como JSON dentro de
///   Product) para permitir reuso entre produtos (uma "Borda" serve
///   todas as pizzas).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddonGroup {
    #[serde(flatten)]
    pub base: BaseFields,
    pub name: String,
    /// Regra de seleção: `"single"` (radio, escolhe 1) ou `"multi"`
    /// (checkbox, escolhe vários). Validado no service.
    pub selection: String,
    /// Mínimo de itens a selecionar. `0` = grupo opcional.
    #[serde(default)]
    pub min_select: i32,
    /// Máximo de itens. `0` = sem teto (faz sentido em `multi`); em
    /// `single` é sempre tratado como 1 mesmo se vier outro valor.
    #[serde(default)]
    pub max_select: i32,
    /// Ordem de exibição relativa ao produto/cadastro (asc).
    #[serde(default)]
    pub sort_order: i32,
}

impl AddonGroup {
    pub fn new(
        company_id: Uuid,
        name: String,
        selection: String,
        min_select: i32,
        max_select: i32,
    ) -> Self {
        Self {
            base: BaseFields::new(company_id),
            name,
            selection,
            min_select,
            max_select,
            sort_order: 0,
        }
    }
}
