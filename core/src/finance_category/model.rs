use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::BaseFields;

/// Em qual aba a categoria aparece no formulário de lançamento.
///
/// - `Payable`: categoria oferecida em "Nova conta a pagar"
///   (ex.: "Aluguel", "Insumos", "Impostos").
/// - `Receivable`: oferecida em "Nova conta a receber"
///   (ex.: "Venda", "Mensalidade", "Serviço").
/// - `Both`: ambas (ex.: "Outros", "Ajuste manual").
///
/// Regra de modelagem (AI_RULES.md §6, §8): tipo claro, descritivo,
/// sem booleanos paralelos (`is_payable` + `is_receivable`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FinanceCategoryScope {
    Payable,
    Receivable,
    #[default]
    Both,
}

impl fmt::Display for FinanceCategoryScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Payable => write!(f, "payable"),
            Self::Receivable => write!(f, "receivable"),
            Self::Both => write!(f, "both"),
        }
    }
}

impl FinanceCategoryScope {
    /// Decodifica a string armazenada no banco. Default `Both` em caso
    /// de valor desconhecido (não perdemos o registro, apenas tratamos
    /// como categoria genérica).
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "payable" => Self::Payable,
            "receivable" => Self::Receivable,
            _ => Self::Both,
        }
    }
}

/// Categoria de lançamento financeiro.
///
/// Regras aplicadas (AI_RULES.md §6, §8):
/// - `BaseFields` obrigatório (UUID, company_id, soft delete, sync).
/// - `name` é o rótulo visível.
/// - `color` (hex `#RRGGBB`) e `icon` (slug do ícone na UI) permitem
///   diferenciação visual no select de categorias.
/// - `scope` decide em qual formulário a categoria aparece.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinanceCategory {
    #[serde(flatten)]
    pub base: BaseFields,
    pub name: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub scope: FinanceCategoryScope,
}

impl FinanceCategory {
    /// Construtor "vazio" — define os defaults seguros. Os campos
    /// concretos (nome, cor, ícone, escopo) são preenchidos pelo
    /// service depois da validação.
    pub fn new(company_id: Uuid, name: String) -> Self {
        Self {
            base: BaseFields::new(company_id),
            name,
            color: String::new(),
            icon: String::new(),
            scope: FinanceCategoryScope::default(),
        }
    }

    /// Indica se a categoria pode ser usada em lançamentos `Payable`.
    pub fn allows_payable(&self) -> bool {
        matches!(
            self.scope,
            FinanceCategoryScope::Payable | FinanceCategoryScope::Both
        )
    }

    /// Indica se a categoria pode ser usada em lançamentos `Receivable`.
    pub fn allows_receivable(&self) -> bool {
        matches!(
            self.scope,
            FinanceCategoryScope::Receivable | FinanceCategoryScope::Both
        )
    }
}
