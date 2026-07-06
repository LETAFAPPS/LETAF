use std::fmt;
use rust_decimal::Decimal;

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::BaseFields;

/// Estado de uma sessão de caixa.
///
/// Regras aplicadas (AI_RULES.md §6, §8):
/// - `Open`: caixa em operação; uma única por `company_id` a cada
///   momento (regra do service, não constraint do banco).
/// - `Closed`: encerrada; valores informados no fechamento ficam
///   imutáveis pra fim de auditoria.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Open,
    Closed,
}

impl fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionStatus::Open => write!(f, "open"),
            SessionStatus::Closed => write!(f, "closed"),
        }
    }
}

impl SessionStatus {
    // `from_str` infalível (default em valor desconhecido); não é o
    // `FromStr` da std, que retorna `Result` — silenciamos o lint.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "open" => SessionStatus::Open,
            _ => SessionStatus::Closed,
        }
    }
}

/// Tipo de movimentação dentro de uma sessão.
///
/// - `Opening`: lançamento automático no `open_session` com o troco
///   inicial; permite reconstituir o saldo em dinheiro replayando os
///   movimentos.
/// - `Sale`: gerado pelo PDV quando uma venda é registrada na sessão
///   ativa. `method` identifica a forma de pagamento.
/// - `Sangria`: retirada manual (saída).
/// - `Suprimento`: entrada manual (reforço de troco, transferência).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MovementKind {
    Opening,
    Sale,
    Sangria,
    Suprimento,
}

impl fmt::Display for MovementKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MovementKind::Opening => write!(f, "opening"),
            MovementKind::Sale => write!(f, "sale"),
            MovementKind::Sangria => write!(f, "sangria"),
            MovementKind::Suprimento => write!(f, "suprimento"),
        }
    }
}

impl MovementKind {
    // `from_str` infalível (default em valor desconhecido); não é o
    // `FromStr` da std, que retorna `Result` — silenciamos o lint.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "opening" => MovementKind::Opening,
            "sale" => MovementKind::Sale,
            "sangria" => MovementKind::Sangria,
            "suprimento" => MovementKind::Suprimento,
            _ => MovementKind::Sangria,
        }
    }
}

/// Sessão de caixa (uma "abertura → fechamento").
///
/// Regras aplicadas (AI_RULES.md §1, §6, §11):
/// - Imutável após fechamento (service rejeita updates em Closed).
/// - `initial_change` é o troco informado na abertura.
/// - `counted_cash` só é preenchido no fechamento.
/// - `difference = counted_cash - expected_cash_at_close` (calculado
///   pelo service no `close_session` — não armazenado, mas preservado
///   na `close_notes` se houver discrepância).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CashSession {
    pub base: BaseFields,
    pub operator_id: Uuid,
    /// Nome do operador no momento da abertura (snapshot — usuário
    /// pode trocar de nome depois).
    pub operator_name: String,
    pub opened_at: NaiveDateTime,
    pub closed_at: Option<NaiveDateTime>,
    pub initial_change: Decimal,
    pub counted_cash: Option<Decimal>,
    pub status: SessionStatus,
    pub open_notes: Option<String>,
    pub close_notes: Option<String>,
}

impl CashSession {
    pub fn new(
        company_id: Uuid,
        operator_id: Uuid,
        operator_name: String,
        initial_change: Decimal,
        open_notes: Option<String>,
    ) -> Self {
        let base = BaseFields::new(company_id);
        Self {
            opened_at: base.created_at,
            base,
            operator_id,
            operator_name,
            closed_at: None,
            initial_change,
            counted_cash: None,
            status: SessionStatus::Open,
            open_notes,
            close_notes: None,
        }
    }
}

/// Movimento individual dentro de uma sessão (livro-razão).
///
/// Regras aplicadas (AI_RULES.md §6, §7):
/// - `amount` sempre **positivo**; o sinal é inferido pelo `kind`
///   (Opening/Sale/Suprimento somam, Sangria subtrai). Centraliza
///   regra no `effective_signed_amount` pra evitar entradas duplicadas.
/// - `method` só preenchido em `Sale` (cash/credit/debit/pix).
/// - `order_id` referencia o pedido quando `kind == Sale`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CashMovement {
    pub base: BaseFields,
    pub session_id: Uuid,
    pub kind: MovementKind,
    pub amount: Decimal,
    pub method: Option<String>,
    pub reason: String,
    pub detail: Option<String>,
    pub order_id: Option<Uuid>,
}

impl CashMovement {
    pub fn new(
        company_id: Uuid,
        session_id: Uuid,
        kind: MovementKind,
        amount: Decimal,
        method: Option<String>,
        reason: String,
        detail: Option<String>,
        order_id: Option<Uuid>,
    ) -> Self {
        Self {
            base: BaseFields::new(company_id),
            session_id,
            kind,
            amount: amount.abs(),
            method,
            reason,
            detail,
            order_id,
        }
    }

    /// Valor com sinal aplicado pelo `kind` — usado em cálculos de
    /// saldo. Sangria é única saída no domínio atual.
    pub fn effective_signed_amount(&self) -> Decimal {
        match self.kind {
            MovementKind::Sangria => -self.amount,
            _ => self.amount,
        }
    }
}

/// Resumo agregado de uma sessão — calculado pelo service em cima do
/// livro-razão (`CashMovement`). Usado na UI (cards do dashboard) e no
/// modal de fechamento.
///
/// Regras aplicadas (AI_RULES.md §1, §14):
/// - UI nunca faz essa agregação — recebe pronto.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionSummary {
    pub sales_total: Decimal,
    pub sales_count: i64,
    /// Total por método de pagamento (somente vendas).
    /// Chaves: "cash", "credit", "debit", "pix".
    pub by_method: std::collections::BTreeMap<String, MethodTotals>,
    pub sangria_total: Decimal,
    pub sangria_count: i64,
    pub suprimento_total: Decimal,
    pub suprimento_count: i64,
    /// Saldo em dinheiro esperado AGORA: initial_change + vendas em
    /// dinheiro + suprimentos − sangrias.
    pub cash_expected: Decimal,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct MethodTotals {
    pub amount: Decimal,
    pub count: i64,
}
