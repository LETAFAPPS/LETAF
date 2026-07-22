//! Analytics financeiro: agregações puras sobre lançamentos e vendas (§3 — a
//! regra de negócio vive aqui, não na UI). Dinheiro em `Decimal` (exato); a UI
//! consome estes números e cuida só de formatação, cores e geometria.
//!
//! Extraído de `desktop/src/ui/finance/snapshot.rs`, que somava em `f64` e
//! misturava cálculo com apresentação.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use rust_decimal::Decimal;

use super::model::{FinanceEntry, FinanceKind, FinanceStatus};
use crate::order::model::{Order, OrderStatus};

// ── KPIs ────────────────────────────────────────────────────────────────

/// Totais de contas ABERTAS (não liquidadas, não canceladas) por natureza,
/// mais o recorte de vencidas. Base dos KPIs e das abas de Financeiro.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FinanceSummary {
    pub to_receive: Decimal,
    pub to_pay: Decimal,
    pub overdue: Decimal,
    pub overdue_count: u32,
    pub count_receivable_open: u32,
    pub count_payable_open: u32,
}

impl FinanceSummary {
    /// Saldo previsto = a receber − a pagar (contas abertas).
    pub fn expected_balance(&self) -> Decimal {
        self.to_receive - self.to_pay
    }
}

/// Agrega os KPIs sobre o conjunto INTEIRO de lançamentos (sem filtro de
/// aba/busca). Ignora liquidados e cancelados; conta vencidas por `is_overdue`.
pub fn summary(entries: &[FinanceEntry], today: NaiveDate) -> FinanceSummary {
    let mut s = FinanceSummary::default();
    for e in entries {
        if e.status == FinanceStatus::Cancelled || e.status.is_settled() {
            continue;
        }
        match e.kind {
            FinanceKind::Receivable => {
                s.to_receive += e.amount;
                s.count_receivable_open += 1;
            }
            FinanceKind::Payable => {
                s.to_pay += e.amount;
                s.count_payable_open += 1;
            }
        }
        if e.is_overdue(today) {
            s.overdue += e.amount;
            s.overdue_count += 1;
        }
    }
    s
}

// ── Fluxo de caixa (projeção) ────────────────────────────────────────────

/// Um dia da projeção de fluxo de caixa (números; a UI formata).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CashFlowDay {
    pub date: NaiveDate,
    pub inflow: Decimal,
    pub outflow: Decimal,
    /// Saldo cumulativo desde o 1º dia da janela (começa em 0 — gráfico
    /// relativo; o saldo absoluto vem do `FinanceSummary`).
    pub balance: Decimal,
}

/// Projeção de fluxo de caixa para `days` dias a partir de `today` (inclusive).
///
/// - **Entradas**: receivables liquidados no dia da baixa + vendas (`Order`)
///   pagas com `payment_method` no dia do pedido (PDV conta como entrada).
/// - **Saídas**: payables liquidados no dia da baixa OU, se pendentes, na
///   `due_date` (estimativa de saída futura).
pub fn cash_flow(
    entries: &[FinanceEntry],
    orders: &[Order],
    today: NaiveDate,
    days: i64,
) -> Vec<CashFlowDay> {
    let span = days.max(1) as usize;
    let mut inflow = vec![Decimal::ZERO; span];
    let mut outflow = vec![Decimal::ZERO; span];

    let idx_of = |d: NaiveDate| -> Option<usize> {
        let i = (d - today).num_days();
        (0..span as i64).contains(&i).then_some(i as usize)
    };

    for e in entries {
        if e.status == FinanceStatus::Cancelled {
            continue;
        }
        // Liquidado → dia da baixa; senão → previsão na due_date.
        let day = if e.status.is_settled() {
            match e.paid_at {
                Some(p) => p.date(),
                None => continue,
            }
        } else {
            e.due_date
        };
        if let Some(i) = idx_of(day) {
            match e.kind {
                FinanceKind::Receivable => inflow[i] += e.amount,
                FinanceKind::Payable => outflow[i] += e.amount,
            }
        }
    }

    // Vendas pagas do PDV — entrada no dia do pedido.
    for o in orders {
        if o.base.deleted_at.is_some() || o.status == OrderStatus::Cancelled {
            continue;
        }
        if o.payment_method.is_none() {
            continue;
        }
        if let Some(i) = idx_of(o.base.created_at.date()) {
            inflow[i] += o.total;
        }
    }

    let mut acc = Decimal::ZERO;
    (0..span)
        .map(|i| {
            acc += inflow[i] - outflow[i];
            CashFlowDay {
                date: today + chrono::Duration::days(i as i64),
                inflow: inflow[i],
                outflow: outflow[i],
                balance: acc,
            }
        })
        .collect()
}

// ── Calendário (agregado por dia de vencimento) ──────────────────────────

/// Agregado de um dia do calendário financeiro (números; a UI monta o grid).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FinanceDayAgg {
    pub count: u32,
    pub inflow: Decimal,
    pub outflow: Decimal,
}

impl FinanceDayAgg {
    /// Líquido do dia = entradas − saídas.
    pub fn net(&self) -> Decimal {
        self.inflow - self.outflow
    }
}

/// Agrega lançamentos por `due_date` (ignora cancelados e removidos).
/// `BTreeMap` → iteração determinística por data.
pub fn day_aggregates(entries: &[FinanceEntry]) -> BTreeMap<NaiveDate, FinanceDayAgg> {
    let mut by_day: BTreeMap<NaiveDate, FinanceDayAgg> = BTreeMap::new();
    for e in entries {
        if e.status == FinanceStatus::Cancelled || e.base.deleted_at.is_some() {
            continue;
        }
        let agg = by_day.entry(e.due_date).or_default();
        agg.count += 1;
        match e.kind {
            FinanceKind::Receivable => agg.inflow += e.amount,
            FinanceKind::Payable => agg.outflow += e.amount,
        }
    }
    by_day
}
