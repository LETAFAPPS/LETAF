//! Tela "Carteira" — tesouraria do estabelecimento.
//!
//! Regras (AI_RULES.md §1, §3, §14): a UI só renderiza; todo o cálculo
//! vive aqui. O saldo é DERIVADO (não há razão duplicada): saldo
//! inicial + entradas − saídas, consolidando as fontes existentes —
//! evita dupla contagem e inconsistência entre telas.
//!
//! Modelo de fluxo (dinheiro REAL entrando/saindo do estabelecimento):
//! - entram: pedidos PAGOS com forma ≠ carteira (não cancelados),
//!   depósitos de clientes na carteira (inclui quitação de fiado),
//!   Financeiro Recebido EXCETO fiado automático (o fiado entra pelo
//!   depósito — contar os dois duplicaria) e os APORTES manuais;
//! - saem: saques de clientes, Financeiro Pago e as RETIRADAS manuais;
//! - ajustes manuais de carteira de cliente seguem o sinal do valor.
//!
//! Pedidos pagos com carteira NÃO contam: consomem crédito que já
//! entrou no depósito do cliente.
//!
//! O SALDO considera todo o histórico; entradas/saídas e o mini-gráfico
//! mostram o DIA corrente, e a lista traz as 10 últimas movimentações.

use std::sync::Arc;

use chrono::{Local, NaiveDate, NaiveDateTime};
use rust_decimal::Decimal;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use letaf_core::finance::model::{FinanceKind, FinanceStatus};
use letaf_core::order::model::OrderStatus;
use letaf_core::treasury::model::TreasuryMovementKind;
use letaf_core::wallet::model::WalletMovementKind;

use crate::context::DesktopState;
use crate::format::{format_order_date, format_order_time, money_br};
use crate::{MainWindow, TreasuryChartBar, TreasuryMovementRow, TreasurySummary};

use super::helpers::{friendly_error, show_toast};

/// Quantas movimentações a lista mostra.
const MOVEMENTS_LIMIT: usize = 10;

pub(crate) fn setup_treasury(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<tokio::sync::Notify>,
) {
    setup_refresh(ui, state, handle);
    setup_open(ui, state, handle, sync_notify.clone());
    setup_modal(ui, state, handle, sync_notify);
}

// ── Snapshot (Send-safe) ────────────────────────────────────────

struct SummaryRaw {
    has_account: bool,
    balance: Decimal,
    initial: Decimal,
    inflow: Decimal,
    outflow: Decimal,
    orders_in: Decimal,
    deposits_in: Decimal,
    finance_in: Decimal,
    finance_out: Decimal,
    withdrawals_out: Decimal,
    manual_in: Decimal,
    manual_out: Decimal,
    movements_count: i32,
    goal: Decimal,
    reserved: Decimal,
}

struct MovementRaw {
    title: String,
    source: String,
    at: NaiveDateTime,
    amount: Decimal,
    positive: bool,
}

fn setup_refresh(ui: &MainWindow, state: &DesktopState, handle: &tokio::runtime::Handle) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_treasury_refresh(move || {
        reapply(&ui_weak, &state, &handle);
    });
}

/// Recarrega o snapshot e aplica na UI (usado pelo refresh e por toda
/// operação que muda o saldo).
fn reapply(
    ui_weak: &slint::Weak<MainWindow>,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
) {
    let ui_weak = ui_weak.clone();
    let state = state.clone();
    handle.spawn(async move {
        let (summary, movements, chart) = build_snapshot(&state).await;
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            apply_to_ui(&ui, &summary, &movements, &chart);
        });
    });
}

/// Primeiro dia do mês da data informada.
fn first_of_month(day: NaiveDate) -> NaiveDate {
    use chrono::Datelike;
    NaiveDate::from_ymd_opt(day.year(), day.month(), 1).unwrap_or(day)
}

async fn build_snapshot(
    state: &DesktopState,
) -> (SummaryRaw, Vec<MovementRaw>, Vec<TreasuryChartBar>) {
    let cid = state.company_id();
    let treasury = state.treasury_service.find(cid).await.ok().flatten();
    let Some(treasury) = treasury else {
        return (empty_summary(), Vec::new(), Vec::new());
    };

    let mut movements: Vec<MovementRaw> = Vec::new();

    // Pedidos pagos (dinheiro novo — forma ≠ carteira).
    let orders = state.order_service.find_all(cid).await.unwrap_or_default();
    for o in orders.iter().filter(|o| {
        o.paid
            && o.payment_method.as_deref() != Some("wallet")
            && o.status != OrderStatus::Cancelled
    }) {
        movements.push(MovementRaw {
            title: format!("Pedido #{:04}", o.number),
            source: "Pedido".into(),
            at: o.base.created_at,
            amount: o.total,
            positive: true,
        });
    }

    // Carteiras de clientes: depósitos entram, saques saem, ajustes
    // seguem o sinal. Cobrança/estorno/limite são movimentos internos
    // de crédito — não são dinheiro novo.
    let accounts = state
        .wallet_service
        .find_all_accounts(cid)
        .await
        .unwrap_or_default();
    for account in &accounts {
        let list = state
            .wallet_service
            .find_movements(cid, account.base.id, 9_999)
            .await
            .unwrap_or_default();
        for m in list {
            match m.kind {
                WalletMovementKind::Deposit => movements.push(MovementRaw {
                    title: m.notes.clone().unwrap_or_else(|| "Depósito de cliente".into()),
                    source: "Carteira".into(),
                    at: m.base.created_at,
                    amount: m.amount,
                    positive: true,
                }),
                WalletMovementKind::Withdraw => movements.push(MovementRaw {
                    title: m.notes.clone().unwrap_or_else(|| "Saque de cliente".into()),
                    source: "Carteira".into(),
                    at: m.base.created_at,
                    amount: m.amount,
                    positive: false,
                }),
                WalletMovementKind::ManualAdjust => movements.push(MovementRaw {
                    title: m.notes.clone().unwrap_or_else(|| "Ajuste de carteira".into()),
                    source: "Carteira".into(),
                    at: m.base.created_at,
                    amount: m.amount.abs(),
                    positive: m.amount >= Decimal::ZERO,
                }),
                WalletMovementKind::OrderCharge
                | WalletMovementKind::OrderRefund
                | WalletMovementKind::LimitChange => {}
            }
        }
    }

    // Financeiro: recebimentos (exceto fiado automático — já contado
    // pelo depósito) e pagamentos liquidados.
    let entries = state.finance_service.find_all(cid).await.unwrap_or_default();
    for e in &entries {
        let is_fiado =
            e.notes.as_deref() == Some(letaf_core::finance::service::FIADO_AUTO_TAG);
        match (e.kind, e.status) {
            (FinanceKind::Receivable, FinanceStatus::Received) if !is_fiado => {
                movements.push(MovementRaw {
                    title: e.description.clone(),
                    source: "Financeiro".into(),
                    at: e.paid_at.unwrap_or(e.base.updated_at),
                    amount: e.amount,
                    positive: true,
                })
            }
            (FinanceKind::Payable, FinanceStatus::Paid) => movements.push(MovementRaw {
                title: e.description.clone(),
                source: "Financeiro".into(),
                at: e.paid_at.unwrap_or(e.base.updated_at),
                amount: e.amount,
                positive: false,
            }),
            _ => {}
        }
    }

    // Aportes / retiradas manuais do caixa.
    let manual = state
        .treasury_service
        .find_movements(cid, 9_999)
        .await
        .unwrap_or_default();
    for m in &manual {
        let positive = m.kind == TreasuryMovementKind::Deposit;
        movements.push(MovementRaw {
            title: m.notes.clone().unwrap_or_else(|| {
                if positive { "Depósito no caixa".into() } else { "Retirada do caixa".into() }
            }),
            source: "Caixa".into(),
            at: m.base.created_at,
            amount: m.amount,
            positive,
        });
    }

    movements.sort_by_key(|m| std::cmp::Reverse(m.at));

    // ── Totais ──
    // O SALDO usa todo o histórico; entradas/saídas e o detalhamento
    // mostram o DIA corrente.
    let today = Local::now().date_naive();
    let in_period = |m: &MovementRaw| m.at.date() == today;

    let sum = |f: &dyn Fn(&MovementRaw) -> bool| -> Decimal {
        movements.iter().filter(|m| f(m)).map(|m| m.amount).sum()
    };
    let by_source = |source: &str, positive: bool| -> Decimal {
        movements
            .iter()
            .filter(|m| m.source == source && m.positive == positive && in_period(m))
            .map(|m| m.amount)
            .sum()
    };

    let total_in = sum(&|m| m.positive);
    let total_out = sum(&|m| !m.positive);
    let balance = treasury.initial_balance + total_in - total_out;

    let inflow = sum(&|m| m.positive && in_period(m));
    let outflow = sum(&|m| !m.positive && in_period(m));

    // Reserva do mês (progresso da meta): resultado líquido do mês.
    let month_start = first_of_month(today);
    let month_in: Decimal = movements
        .iter()
        .filter(|m| m.positive && m.at.date() >= month_start)
        .map(|m| m.amount)
        .sum();
    let month_out: Decimal = movements
        .iter()
        .filter(|m| !m.positive && m.at.date() >= month_start)
        .map(|m| m.amount)
        .sum();

    let summary = SummaryRaw {
        has_account: true,
        balance,
        initial: treasury.initial_balance,
        inflow,
        outflow,
        orders_in: by_source("Pedido", true),
        deposits_in: by_source("Carteira", true),
        finance_in: by_source("Financeiro", true),
        finance_out: by_source("Financeiro", false),
        withdrawals_out: by_source("Carteira", false),
        manual_in: by_source("Caixa", true),
        manual_out: by_source("Caixa", false),
        movements_count: movements.len() as i32,
        goal: treasury.reserve_goal,
        reserved: month_in - month_out,
    };

    let chart = build_chart(&movements, today);

    movements.truncate(MOVEMENTS_LIMIT);
    (summary, movements, chart)
}

fn empty_summary() -> SummaryRaw {
    SummaryRaw {
        has_account: false,
        balance: Decimal::ZERO,
        initial: Decimal::ZERO,
        inflow: Decimal::ZERO,
        outflow: Decimal::ZERO,
        orders_in: Decimal::ZERO,
        deposits_in: Decimal::ZERO,
        finance_in: Decimal::ZERO,
        finance_out: Decimal::ZERO,
        withdrawals_out: Decimal::ZERO,
        manual_in: Decimal::ZERO,
        manual_out: Decimal::ZERO,
        movements_count: 0,
        goal: Decimal::ZERO,
        reserved: Decimal::ZERO,
    }
}

/// Mini-gráfico do card de saldo: o dia dividido em 14 faixas, cada uma
/// com o movimento líquido (entradas − saídas) do intervalo. Só a MAIOR
/// barra recebe destaque.
fn build_chart(movements: &[MovementRaw], today: NaiveDate) -> Vec<TreasuryChartBar> {
    const SLOTS: usize = 14;
    use chrono::Timelike;
    use rust_decimal::prelude::ToPrimitive;

    let mut totals = vec![0.0_f64; SLOTS];
    for m in movements.iter().filter(|m| m.at.date() == today) {
        let idx = (m.at.hour() as usize * SLOTS / 24).min(SLOTS - 1);
        let v = m.amount.to_f64().unwrap_or(0.0);
        totals[idx] += if m.positive { v } else { -v };
    }

    let max = totals.iter().copied().fold(0.0_f64, |a, b| a.max(b.abs()));
    // Índice da maior barra — a única destacada; nenhuma quando tudo é zero.
    let peak = totals
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.abs().total_cmp(&b.abs()))
        .map(|(i, _)| i)
        .filter(|_| max > 0.0);

    totals
        .into_iter()
        .enumerate()
        .map(|(i, v)| TreasuryChartBar {
            progress: if max > 0.0 { (v.abs() / max) as f32 } else { 0.0 },
            highlight: peak == Some(i),
        })
        .collect()
}

fn apply_to_ui(
    ui: &MainWindow,
    s: &SummaryRaw,
    movements: &[MovementRaw],
    chart: &[TreasuryChartBar],
) {
    // Saldo dividido em "R$ 1.103" + ",50" para o destaque do card.
    let balance_full = money_br(s.balance);
    let (main, cents) = match balance_full.rfind(',') {
        Some(pos) => (balance_full[..pos].to_string(), balance_full[pos..].to_string()),
        None => (balance_full.clone(), String::new()),
    };
    let net = s.inflow - s.outflow;
    use rust_decimal::prelude::ToPrimitive;
    let goal_progress = if s.goal > Decimal::ZERO {
        (s.reserved.max(Decimal::ZERO) / s.goal).to_f64().unwrap_or(0.0).clamp(0.0, 1.0) as f32
    } else {
        0.0
    };
    let goal_display = if s.goal > Decimal::ZERO {
        format!(
            "{} de {} · {:.0}%",
            money_br(s.reserved.max(Decimal::ZERO)),
            money_br(s.goal),
            goal_progress * 100.0
        )
    } else {
        "Sem meta definida — toque no lápis para configurar".to_string()
    };

    ui.set_treasury_summary(TreasurySummary {
        has_account: s.has_account,
        balance_main: SharedString::from(main),
        balance_cents: SharedString::from(cents),
        balance_negative: s.balance < Decimal::ZERO,
        balance_display: SharedString::from(balance_full),
        initial_display: SharedString::from(money_br(s.initial)),
        inflow_display: SharedString::from(money_br(s.inflow)),
        outflow_display: SharedString::from(money_br(s.outflow)),
        net_display: SharedString::from(if net < Decimal::ZERO {
            format!("− {}", money_br(-net))
        } else {
            format!("+ {}", money_br(net))
        }),
        net_positive: net >= Decimal::ZERO,
        net_label: SharedString::from("SALDO LÍQUIDO DO DIA"),
        orders_display: SharedString::from(format!("+ {}", money_br(s.orders_in))),
        deposits_display: SharedString::from(format!("+ {}", money_br(s.deposits_in))),
        finance_in_display: SharedString::from(format!("+ {}", money_br(s.finance_in))),
        finance_out_display: SharedString::from(format!("− {}", money_br(s.finance_out))),
        withdrawals_display: SharedString::from(format!("− {}", money_br(s.withdrawals_out))),
        manual_in_display: SharedString::from(format!("+ {}", money_br(s.manual_in))),
        manual_out_display: SharedString::from(format!("− {}", money_br(s.manual_out))),
        movements_count: s.movements_count,
        goal_progress,
        goal_display: SharedString::from(goal_display),
        goal_input: SharedString::from(if s.goal > Decimal::ZERO {
            format!("{:.2}", s.goal).replace('.', ",")
        } else {
            String::new()
        }),
    });

    let rows: Vec<TreasuryMovementRow> = movements
        .iter()
        .map(|m| TreasuryMovementRow {
            title: SharedString::from(m.title.clone()),
            source: SharedString::from(m.source.clone()),
            time_display: SharedString::from(format!(
                "{} · {}",
                format_order_date(m.at),
                format_order_time(m.at)
            )),
            amount_display: SharedString::from(if m.positive {
                format!("+ {}", money_br(m.amount))
            } else {
                format!("− {}", money_br(m.amount))
            }),
            tone: SharedString::from(if m.positive { "pos" } else { "neg" }),
        })
        .collect();
    ui.set_treasury_movements(ModelRc::new(VecModel::from(rows)));
    ui.set_treasury_chart(ModelRc::new(VecModel::from(chart.to_vec())));
}

// ── Cadastro inicial ────────────────────────────────────────────

/// Parse de valor monetário pt-BR ("1.234,56", "5", "R$ 5,00").
fn parse_money_br(s: &str) -> Option<Decimal> {
    use std::str::FromStr;
    let cleaned: String = s
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == ',' || *c == '.' || *c == '-')
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    let normalized = if cleaned.contains(',') {
        cleaned.replace('.', "").replace(',', ".")
    } else {
        cleaned
    };
    Decimal::from_str(&normalized).ok()
}

fn setup_open(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<tokio::sync::Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_treasury_open(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        let raw = ui.get_treasury_initial_input().to_string();
        let notes = ui.get_treasury_notes_input().to_string();
        let Some(initial) = parse_money_br(&raw) else {
            ui.set_treasury_setup_error(SharedString::from("Informe um saldo inicial válido"));
            return;
        };
        ui.set_treasury_setup_error(SharedString::default());
        let notes_opt = if notes.trim().is_empty() { None } else { Some(notes) };
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let notify = sync_notify.clone();
        let handle_inner = handle.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            let result = state.treasury_service.open(cid, initial, notes_opt).await;
            let ui_weak2 = ui_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak2.upgrade() else { return };
                match result {
                    Ok(_) => {
                        notify.notify_one();
                        show_toast(&ui, "Carteira criada", "success");
                    }
                    Err(e) => {
                        ui.set_treasury_setup_error(SharedString::from(friendly_error(&e)));
                    }
                }
            });
            reapply(&ui_weak, &state, &handle_inner);
        });
    });
}

// ── Modal: aporte / retirada / meta ─────────────────────────────

fn setup_modal(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<tokio::sync::Notify>,
) {
    // Abrir: limpa o formulário (a meta já abre com o valor atual).
    let ui_weak = ui.as_weak();
    ui.on_treasury_open_modal(move |kind| {
        let Some(ui) = ui_weak.upgrade() else { return };
        let is_goal = kind == "goal";
        ui.set_treasury_modal_kind(kind);
        ui.set_treasury_modal_amount(if is_goal {
            ui.get_treasury_summary().goal_input
        } else {
            SharedString::default()
        });
        ui.set_treasury_modal_notes(SharedString::default());
        ui.set_treasury_modal_error(SharedString::default());
        ui.set_treasury_show_modal(true);
    });

    let ui_weak = ui.as_weak();
    ui.on_treasury_close_modal(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_treasury_show_modal(false);
        }
    });

    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_treasury_confirm_modal(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        let kind = ui.get_treasury_modal_kind().to_string();
        let raw = ui.get_treasury_modal_amount().to_string();
        let notes = ui.get_treasury_modal_notes().to_string();
        let Some(amount) = parse_money_br(&raw) else {
            ui.set_treasury_modal_error(SharedString::from("Informe um valor válido"));
            return;
        };
        ui.set_treasury_modal_error(SharedString::default());
        let notes_opt = if notes.trim().is_empty() { None } else { Some(notes) };
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let notify = sync_notify.clone();
        let handle_inner = handle.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            let (result, msg) = match kind.as_str() {
                "deposit" => (
                    state
                        .treasury_service
                        .deposit(cid, amount, notes_opt)
                        .await
                        .map(|_| ()),
                    "Depósito registrado",
                ),
                "withdraw" => (
                    state
                        .treasury_service
                        .withdraw(cid, amount, notes_opt)
                        .await
                        .map(|_| ()),
                    "Retirada registrada",
                ),
                _ => (
                    state
                        .treasury_service
                        .set_reserve_goal(cid, amount)
                        .await
                        .map(|_| ()),
                    "Meta atualizada",
                ),
            };
            let ui_weak2 = ui_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak2.upgrade() else { return };
                match result {
                    Ok(()) => {
                        notify.notify_one();
                        ui.set_treasury_show_modal(false);
                        show_toast(&ui, msg, "success");
                    }
                    Err(e) => {
                        ui.set_treasury_modal_error(SharedString::from(friendly_error(&e)));
                    }
                }
            });
            reapply(&ui_weak, &state, &handle_inner);
        });
    });
}
