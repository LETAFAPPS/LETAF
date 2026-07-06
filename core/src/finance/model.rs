use std::fmt;
use rust_decimal::Decimal;

use chrono::{NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::BaseFields;

/// Natureza do lançamento. Decide em qual aba aparece e o sinal no
/// fluxo de caixa.
///
/// - `Payable`: contas a pagar (saída).
/// - `Receivable`: contas a receber (entrada).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinanceKind {
    Payable,
    Receivable,
}

impl fmt::Display for FinanceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Payable => write!(f, "payable"),
            Self::Receivable => write!(f, "receivable"),
        }
    }
}

impl FinanceKind {
    /// Decodifica o `kind` vindo do banco. Default `Payable` em caso
    /// de string desconhecida — escolha conservadora pois trata como
    /// saída (afeta saldo previsto pra menos, nunca pra mais).
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "receivable" => Self::Receivable,
            _ => Self::Payable,
        }
    }
}

/// Tipo da contraparte do lançamento.
///
/// - `Supplier`: fornecedor (típico de `Payable`).
/// - `Customer`: cliente (típico de `Receivable`).
/// - `Other`: contraparte não cadastrada (ex.: "Receita Federal",
///   "Aluguel"); `party_id` fica `None` e usamos só `party_name`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PartyType {
    Supplier,
    Customer,
    #[default]
    Other,
}

impl fmt::Display for PartyType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Supplier => write!(f, "supplier"),
            Self::Customer => write!(f, "customer"),
            Self::Other => write!(f, "other"),
        }
    }
}

impl PartyType {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "supplier" => Self::Supplier,
            "customer" => Self::Customer,
            _ => Self::Other,
        }
    }
}

/// Estado do lançamento.
///
/// - `Pending`: aguardando pagamento/recebimento (default).
/// - `Scheduled`: agendado (data futura confirmada — boleto/débito
///   automático). Visualmente igual a Pending mas comunica intenção.
/// - `Paid` / `Received`: baixado (paid_at preenchido).
/// - `Cancelled`: cancelado (`paid_at` permanece `None`).
///
/// `Overdue` NÃO é persistido: derivamos em runtime via
/// `is_overdue(today)` para evitar batch diário de atualização.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinanceStatus {
    Pending,
    Scheduled,
    Paid,
    Received,
    Cancelled,
}

impl fmt::Display for FinanceStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Scheduled => write!(f, "scheduled"),
            Self::Paid => write!(f, "paid"),
            Self::Received => write!(f, "received"),
            Self::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl FinanceStatus {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "scheduled" => Self::Scheduled,
            "paid" => Self::Paid,
            "received" => Self::Received,
            "cancelled" => Self::Cancelled,
            _ => Self::Pending,
        }
    }

    /// `true` quando o lançamento foi baixado (Paid ou Received).
    pub fn is_settled(self) -> bool {
        matches!(self, Self::Paid | Self::Received)
    }
}

/// Recorrência do lançamento.
///
/// - `Once`: lançamento único.
/// - `Weekly` / `Monthly`: geram entradas filhas (`parent_id`) com
///   datas futuras na criação. O service decide quantas pre-gerar.
/// - `Custom`: reservado para regras avançadas (a definir).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FinanceRecurrence {
    #[default]
    Once,
    Weekly,
    Monthly,
    Custom,
}

impl fmt::Display for FinanceRecurrence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Once => write!(f, "once"),
            Self::Weekly => write!(f, "weekly"),
            Self::Monthly => write!(f, "monthly"),
            Self::Custom => write!(f, "custom"),
        }
    }
}

impl FinanceRecurrence {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "weekly" => Self::Weekly,
            "monthly" => Self::Monthly,
            "custom" => Self::Custom,
            _ => Self::Once,
        }
    }
}

/// Lançamento financeiro — uma conta a pagar ou a receber.
///
/// Regras aplicadas (AI_RULES.md §6):
/// - `BaseFields` (UUID, company_id, soft delete, sync).
/// - `party_name` é snapshot do nome no momento do lançamento; se o
///   cliente/fornecedor for renomeado depois, o histórico não muda.
/// - `parent_id` aponta para a entrada "cabeça" quando este registro
///   é uma parcela ou ocorrência de recorrência. O cabeça aponta
///   pra si mesmo (`parent_id == base.id`) — convenção simples para
///   uniformizar queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinanceEntry {
    #[serde(flatten)]
    pub base: BaseFields,
    pub kind: FinanceKind,
    pub description: String,
    pub party_id: Option<Uuid>,
    pub party_name: String,
    pub party_type: PartyType,
    pub category_id: Option<Uuid>,
    pub amount: Decimal,
    pub due_date: NaiveDate,
    pub paid_at: Option<NaiveDateTime>,
    pub status: FinanceStatus,
    pub payment_method: Option<String>,
    pub notes: Option<String>,
    pub recurrence: FinanceRecurrence,
    pub parent_id: Uuid,
    pub installment_index: i32,
    pub installment_total: i32,
    pub order_id: Option<Uuid>,
}

impl FinanceEntry {
    /// Construtor mínimo — service preenche o restante. `parent_id`
    /// aponta pra si mesmo: assim entradas "isoladas" e "cabeça de
    /// grupo" têm o mesmo formato (a query `parent_id = base.id` pega
    /// as cabeças e a query `parent_id = X AND base.id != X` pega
    /// somente filhas).
    pub fn new(
        company_id: Uuid,
        kind: FinanceKind,
        description: String,
        amount: Decimal,
        due_date: NaiveDate,
    ) -> Self {
        let base = BaseFields::new(company_id);
        let id = base.id;
        Self {
            base,
            kind,
            description,
            party_id: None,
            party_name: String::new(),
            party_type: PartyType::Other,
            category_id: None,
            amount,
            due_date,
            paid_at: None,
            status: FinanceStatus::Pending,
            payment_method: None,
            notes: None,
            recurrence: FinanceRecurrence::Once,
            parent_id: id,
            installment_index: 1,
            installment_total: 1,
            order_id: None,
        }
    }

    /// `true` quando o lançamento venceu mas ainda não foi liquidado
    /// nem cancelado. Não persistimos esse estado — derivamos sempre
    /// que precisamos exibir.
    pub fn is_overdue(&self, today: NaiveDate) -> bool {
        matches!(self.status, FinanceStatus::Pending | FinanceStatus::Scheduled)
            && self.due_date < today
    }

    /// `true` quando este registro é uma parcela/ocorrência (não a
    /// cabeça do grupo).
    pub fn is_installment_child(&self) -> bool {
        self.parent_id != self.base.id
    }
}
