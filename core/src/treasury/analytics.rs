//! Consolidação da tesouraria (domínio puro, §1/§3): o que é DINHEIRO
//! NOVO entrando ou saindo do estabelecimento.
//!
//! Extraído de `desktop/src/ui/treasury/mod.rs`, onde a UI decidia quais
//! movimentos contavam. As regras (preservadas na íntegra):
//!
//! - ENTRAM: pedidos PAGOS com forma ≠ carteira (não cancelados),
//!   depósitos de clientes na carteira (inclui quitação de fiado),
//!   Financeiro Recebido EXCETO o fiado automático (o fiado entra pelo
//!   depósito — contar os dois duplicaria) e os APORTES manuais.
//! - SAEM: saques de clientes, Financeiro Pago e as RETIRADAS manuais.
//! - Ajuste manual de carteira segue o SINAL do valor.
//! - Pedido pago com carteira NÃO conta: consome crédito que já entrou
//!   pelo depósito.
//! - Cobrança/estorno de pedido, mudança de limite e cobrança/baixa de
//!   conta a receber são ESPELHOS CONTÁBEIS da carteira do cliente —
//!   não são dinheiro entrando ou saindo do caixa.
//! - NADA anterior à ABERTURA da carteira entra: o saldo inicial já
//!   resume o que existia até ali.
//!
//! O SALDO considera todo o histórico; entradas/saídas e o detalhamento
//! mostram o DIA corrente no fuso da loja (§6 — `created_at` é UTC).
//!
//! Determinístico: a data de referência entra por parâmetro (sem
//! relógio), o que torna a consolidação testável.

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Timelike};
use rust_decimal::Decimal;

use super::model::{Treasury, TreasuryMovement, TreasuryMovementKind};
use crate::finance::model::{FinanceEntry, FinanceKind, FinanceStatus};
use crate::finance::service::FIADO_AUTO_TAG;
use crate::order::model::{Order, OrderStatus};
use crate::wallet::model::{WalletMovement, WalletMovementKind};

/// Origem do dinheiro — agrupa o detalhamento do dia.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CashSource {
    /// Venda paga em dinheiro/cartão/PIX.
    Order,
    /// Carteira de um cliente (depósito, saque ou ajuste).
    Wallet,
    /// Lançamento do Financeiro liquidado.
    Finance,
    /// Aporte/retirada manual do caixa.
    Cashbox,
}

/// O que exatamente aconteceu — a UI rotula a partir daqui (§3: o
/// texto em pt-BR é apresentação, a classificação é domínio).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CashDetail {
    /// Pedido pago com forma ≠ carteira; carrega o número do pedido.
    OrderPayment { number: i64 },
    WalletDeposit,
    WalletWithdraw,
    /// Ajuste manual da carteira de um cliente (sinal em `positive`).
    WalletAdjust,
    FinanceReceived,
    FinancePaid,
    CashboxDeposit,
    CashboxWithdraw,
}

impl CashDetail {
    /// Origem à qual este movimento pertence no detalhamento.
    pub fn source(self) -> CashSource {
        match self {
            Self::OrderPayment { .. } => CashSource::Order,
            Self::WalletDeposit | Self::WalletWithdraw | Self::WalletAdjust => CashSource::Wallet,
            Self::FinanceReceived | Self::FinancePaid => CashSource::Finance,
            Self::CashboxDeposit | Self::CashboxWithdraw => CashSource::Cashbox,
        }
    }
}

/// Um movimento de caixa já classificado (valor SEMPRE positivo; a
/// direção está em `positive`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CashMovement {
    pub detail: CashDetail,
    /// Texto já existente no dado (notes/description), quando houver.
    pub description: Option<String>,
    /// Instante do movimento em UTC (como gravado).
    pub at: NaiveDateTime,
    pub amount: Decimal,
    pub positive: bool,
}

impl CashMovement {
    pub fn source(&self) -> CashSource {
        self.detail.source()
    }
}

/// Detalhamento do DIA por origem — um card por linha na UI.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DayBreakdown {
    pub orders_in: Decimal,
    pub wallet_in: Decimal,
    pub finance_in: Decimal,
    pub cashbox_in: Decimal,
    pub finance_out: Decimal,
    pub wallet_out: Decimal,
    pub cashbox_out: Decimal,
}

/// Retrato consolidado da tesouraria.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreasurySnapshot {
    pub initial: Decimal,
    /// Saldo derivado: inicial + entradas − saídas (todo o histórico
    /// posterior à abertura).
    pub balance: Decimal,
    /// Entradas e saídas do DIA corrente.
    pub inflow: Decimal,
    pub outflow: Decimal,
    pub breakdown: DayBreakdown,
    /// Meta de reserva mensal (0 = sem meta).
    pub goal: Decimal,
    /// Resultado líquido do MÊS corrente (progresso da meta).
    pub reserved: Decimal,
    /// Todos os movimentos, do mais recente para o mais antigo.
    pub movements: Vec<CashMovement>,
}

/// Fontes já carregadas do banco pela camada de aplicação (§10 — o
/// domínio não acessa repositório).
pub struct CashSources<'a> {
    pub treasury: &'a Treasury,
    pub orders: &'a [Order],
    pub wallet_movements: &'a [WalletMovement],
    pub finance_entries: &'a [FinanceEntry],
    pub cashbox_movements: &'a [TreasuryMovement],
}

/// Consolida todas as fontes num único retrato do caixa.
///
/// `today` é a data JÁ no fuso da loja (recorte do dia e do mês).
pub fn consolidate(sources: &CashSources<'_>, today: NaiveDate) -> TreasurySnapshot {
    let mut movements = Vec::new();
    collect_orders(sources.orders, &mut movements);
    collect_wallet(sources.wallet_movements, &mut movements);
    collect_finance(sources.finance_entries, &mut movements);
    collect_cashbox(sources.cashbox_movements, &mut movements);

    // Nada anterior à ABERTURA entra: o saldo inicial já representa o
    // dinheiro que existia até ali (contar de novo duplicaria).
    let opened_at = sources.treasury.base.created_at;
    movements.retain(|m| m.at >= opened_at);
    movements.sort_by_key(|m| std::cmp::Reverse(m.at));

    let initial = sources.treasury.initial_balance;
    let total_in = sum(&movements, |m| m.positive);
    let total_out = sum(&movements, |m| !m.positive);
    let month_start = first_of_month(today);

    TreasurySnapshot {
        initial,
        balance: initial + total_in - total_out,
        inflow: sum(&movements, |m| m.positive && on_day(m, today)),
        outflow: sum(&movements, |m| !m.positive && on_day(m, today)),
        breakdown: day_breakdown(&movements, today),
        goal: sources.treasury.reserve_goal,
        reserved: sum(&movements, |m| m.positive && since(m, month_start))
            - sum(&movements, |m| !m.positive && since(m, month_start)),
        movements,
    }
}

// ── Classificação por fonte ──────────────────────────────────────────────

/// Pedidos pagos com forma ≠ carteira: dinheiro novo no caixa.
fn collect_orders(orders: &[Order], out: &mut Vec<CashMovement>) {
    for o in orders.iter().filter(|o| {
        o.paid
            && o.payment_method.as_deref() != Some("wallet")
            && o.status != OrderStatus::Cancelled
    }) {
        out.push(CashMovement {
            detail: CashDetail::OrderPayment { number: o.number },
            description: None,
            at: o.base.created_at,
            amount: o.total,
            positive: true,
        });
    }
}

/// Carteiras de clientes: depósito entra, saque sai, ajuste segue o
/// sinal. Os demais tipos são espelhos contábeis (ver doc do módulo).
fn collect_wallet(list: &[WalletMovement], out: &mut Vec<CashMovement>) {
    for m in list {
        let (detail, positive) = match m.kind {
            WalletMovementKind::Deposit => (CashDetail::WalletDeposit, true),
            WalletMovementKind::Withdraw => (CashDetail::WalletWithdraw, false),
            WalletMovementKind::ManualAdjust => {
                (CashDetail::WalletAdjust, m.amount >= Decimal::ZERO)
            }
            WalletMovementKind::OrderCharge
            | WalletMovementKind::OrderRefund
            | WalletMovementKind::LimitChange
            | WalletMovementKind::ReceivableCharge
            | WalletMovementKind::ReceivableSettle => continue,
        };
        out.push(CashMovement {
            detail,
            description: m.notes.clone(),
            at: m.base.created_at,
            // Ajuste guarda o sinal no valor; aqui o valor é sempre
            // positivo e a direção fica em `positive`.
            amount: m.amount.abs(),
            positive,
        });
    }
}

/// Financeiro: recebimentos (exceto o fiado automático, já contado pelo
/// depósito na carteira) e pagamentos liquidados.
fn collect_finance(entries: &[FinanceEntry], out: &mut Vec<CashMovement>) {
    for e in entries {
        let is_fiado = e.notes.as_deref() == Some(FIADO_AUTO_TAG);
        let (detail, positive) = match (e.kind, e.status) {
            (FinanceKind::Receivable, FinanceStatus::Received) if !is_fiado => {
                (CashDetail::FinanceReceived, true)
            }
            (FinanceKind::Payable, FinanceStatus::Paid) => (CashDetail::FinancePaid, false),
            _ => continue,
        };
        out.push(CashMovement {
            detail,
            description: Some(e.description.clone()),
            // Liquidação sem `paid_at` (dado legado) cai no `updated_at`.
            at: e.paid_at.unwrap_or(e.base.updated_at),
            amount: e.amount,
            positive,
        });
    }
}

/// Aportes e retiradas manuais do caixa.
fn collect_cashbox(list: &[TreasuryMovement], out: &mut Vec<CashMovement>) {
    for m in list {
        let positive = m.kind == TreasuryMovementKind::Deposit;
        out.push(CashMovement {
            detail: if positive {
                CashDetail::CashboxDeposit
            } else {
                CashDetail::CashboxWithdraw
            },
            description: m.notes.clone(),
            at: m.base.created_at,
            amount: m.amount,
            positive,
        });
    }
}

// ── Totais ───────────────────────────────────────────────────────────────

/// Detalhamento do dia: uma soma por (origem, direção).
fn day_breakdown(movements: &[CashMovement], today: NaiveDate) -> DayBreakdown {
    let by = |source: CashSource, positive: bool| {
        sum(movements, |m| {
            m.source() == source && m.positive == positive && on_day(m, today)
        })
    };
    DayBreakdown {
        orders_in: by(CashSource::Order, true),
        wallet_in: by(CashSource::Wallet, true),
        finance_in: by(CashSource::Finance, true),
        cashbox_in: by(CashSource::Cashbox, true),
        finance_out: by(CashSource::Finance, false),
        wallet_out: by(CashSource::Wallet, false),
        cashbox_out: by(CashSource::Cashbox, false),
    }
}

fn sum(movements: &[CashMovement], keep: impl Fn(&CashMovement) -> bool) -> Decimal {
    movements.iter().filter(|m| keep(m)).map(|m| m.amount).sum()
}

/// O movimento caiu no dia `day` do RELÓGIO DA LOJA (`at` é UTC).
fn on_day(m: &CashMovement, day: NaiveDate) -> bool {
    crate::tz::to_local(m.at).date() == day
}

/// O movimento é do dia `from` em diante (fuso da loja).
fn since(m: &CashMovement, from: NaiveDate) -> bool {
    crate::tz::to_local(m.at).date() >= from
}

/// Primeiro dia do mês da data informada.
fn first_of_month(day: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(day.year(), day.month(), 1).unwrap_or(day)
}

// ── Série por hora (mini-gráfico do card de saldo) ────────────────────────

/// Entradas e saídas de UMA hora cheia (horário da loja).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HourFlow {
    /// Início da hora, no fuso da loja.
    pub start: NaiveDateTime,
    pub inflow: Decimal,
    pub outflow: Decimal,
}

impl HourFlow {
    /// Líquido da hora: positivo sobe, negativo desce.
    pub fn net(&self) -> Decimal {
        self.inflow - self.outflow
    }
}

/// Últimas `slots` horas cheias terminando em `current_hour`
/// (inclusive), da mais antiga para a mais recente.
pub fn hourly_flow(
    movements: &[CashMovement],
    current_hour: NaiveDateTime,
    slots: usize,
) -> Vec<HourFlow> {
    let mut flows: Vec<HourFlow> = (0..slots)
        .map(|i| HourFlow {
            start: current_hour - Duration::hours((slots - 1 - i) as i64),
            inflow: Decimal::ZERO,
            outflow: Decimal::ZERO,
        })
        .collect();
    for m in movements {
        // `at` é UTC; a janela é contada no relógio da loja.
        let local = crate::tz::to_local(m.at);
        let Some(hour) = local.date().and_hms_opt(local.hour(), 0, 0) else {
            continue;
        };
        let back = (current_hour - hour).num_hours();
        if back < 0 || back >= slots as i64 {
            continue;
        }
        let slot = &mut flows[slots - 1 - back as usize];
        if m.positive {
            slot.inflow += m.amount;
        } else {
            slot.outflow += m.amount;
        }
    }
    flows
}
