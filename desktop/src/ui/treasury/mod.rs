//! Tela "Carteira" — tesouraria do estabelecimento.
//!
//! Regras (AI_RULES.md §1, §3, §14): a UI só renderiza; todo o cálculo
//! vive aqui. O saldo é DERIVADO (não há razão duplicada): saldo
//! inicial + entradas − saídas, consolidando as fontes existentes —
//! evita dupla contagem e inconsistência entre telas.
//!
//! Modelo de fluxo (dinheiro REAL entrando/saindo do estabelecimento):
//! - entram: pedidos PAGOS com forma ≠ carteira (não cancelados),
//!   depósitos de clientes na carteira (inclui quitação de fiado) e
//!   Financeiro Recebido EXCETO fiado automático (o fiado entra pelo
//!   depósito — contar os dois duplicaria);
//! - saem: saques de clientes e Financeiro Pago;
//! - ajustes manuais de carteira seguem o sinal do valor.
//!
//! Pedidos pagos com carteira NÃO contam: consomem crédito que já
//! entrou no depósito do cliente.

use std::sync::Arc;

use rust_decimal::Decimal;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use letaf_core::finance::model::{FinanceKind, FinanceStatus};
use letaf_core::order::model::OrderStatus;
use letaf_core::wallet::model::WalletMovementKind;

use crate::context::DesktopState;
use crate::format::{format_order_date, format_order_time, money_br};
use crate::{MainWindow, TreasuryMovementRow, TreasurySummary};

use super::helpers::{friendly_error, show_toast};

pub(crate) fn setup_treasury(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<tokio::sync::Notify>,
) {
    setup_refresh(ui, state, handle);
    setup_open(ui, state, handle, sync_notify);
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
    movements_count: i32,
}

struct MovementRaw {
    title: String,
    source: String,
    at: chrono::NaiveDateTime,
    amount: Decimal,
    positive: bool,
}

fn setup_refresh(ui: &MainWindow, state: &DesktopState, handle: &tokio::runtime::Handle) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_treasury_refresh(move || {
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        handle.spawn(async move {
            let (summary, movements) = build_snapshot(&state).await;
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                apply_to_ui(&ui, &summary, &movements);
            });
        });
    });
}

async fn build_snapshot(state: &DesktopState) -> (SummaryRaw, Vec<MovementRaw>) {
    let cid = state.company_id();
    let treasury = state.treasury_service.find(cid).await.ok().flatten();
    let Some(treasury) = treasury else {
        return (
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
                movements_count: 0,
            },
            Vec::new(),
        );
    };

    let mut movements: Vec<MovementRaw> = Vec::new();
    let mut orders_in = Decimal::ZERO;
    let mut deposits_in = Decimal::ZERO;
    let mut withdrawals_out = Decimal::ZERO;
    let mut finance_in = Decimal::ZERO;
    let mut finance_out = Decimal::ZERO;

    // Pedidos pagos (dinheiro novo — forma ≠ carteira).
    let orders = state.order_service.find_all(cid).await.unwrap_or_default();
    for o in orders.iter().filter(|o| {
        o.paid
            && o.payment_method.as_deref() != Some("wallet")
            && o.status != OrderStatus::Cancelled
    }) {
        orders_in += o.total;
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
                WalletMovementKind::Deposit => {
                    deposits_in += m.amount;
                    movements.push(MovementRaw {
                        title: m.notes.clone().unwrap_or_else(|| "Depósito de cliente".into()),
                        source: "Carteira".into(),
                        at: m.base.created_at,
                        amount: m.amount,
                        positive: true,
                    });
                }
                WalletMovementKind::Withdraw => {
                    withdrawals_out += m.amount;
                    movements.push(MovementRaw {
                        title: m.notes.clone().unwrap_or_else(|| "Saque de cliente".into()),
                        source: "Carteira".into(),
                        at: m.base.created_at,
                        amount: m.amount,
                        positive: false,
                    });
                }
                WalletMovementKind::ManualAdjust => {
                    let positive = m.amount >= Decimal::ZERO;
                    if positive {
                        deposits_in += m.amount;
                    } else {
                        withdrawals_out += -m.amount;
                    }
                    movements.push(MovementRaw {
                        title: m.notes.clone().unwrap_or_else(|| "Ajuste de carteira".into()),
                        source: "Carteira".into(),
                        at: m.base.created_at,
                        amount: m.amount.abs(),
                        positive,
                    });
                }
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
        let is_fiado = e.notes.as_deref()
            == Some(letaf_core::finance::service::FIADO_AUTO_TAG);
        match (e.kind, e.status) {
            (FinanceKind::Receivable, FinanceStatus::Received) if !is_fiado => {
                finance_in += e.amount;
                movements.push(MovementRaw {
                    title: e.description.clone(),
                    source: "Financeiro".into(),
                    at: e.paid_at.unwrap_or(e.base.updated_at),
                    amount: e.amount,
                    positive: true,
                });
            }
            (FinanceKind::Payable, FinanceStatus::Paid) => {
                finance_out += e.amount;
                movements.push(MovementRaw {
                    title: e.description.clone(),
                    source: "Financeiro".into(),
                    at: e.paid_at.unwrap_or(e.base.updated_at),
                    amount: e.amount,
                    positive: false,
                });
            }
            _ => {}
        }
    }

    movements.sort_by_key(|m| std::cmp::Reverse(m.at));
    let movements_count = movements.len() as i32;
    movements.truncate(50);

    let inflow = orders_in + deposits_in + finance_in;
    let outflow = withdrawals_out + finance_out;
    let balance = treasury.initial_balance + inflow - outflow;

    (
        SummaryRaw {
            has_account: true,
            balance,
            initial: treasury.initial_balance,
            inflow,
            outflow,
            orders_in,
            deposits_in,
            finance_in,
            finance_out,
            withdrawals_out,
            movements_count,
        },
        movements,
    )
}

fn apply_to_ui(ui: &MainWindow, s: &SummaryRaw, movements: &[MovementRaw]) {
    ui.set_treasury_summary(TreasurySummary {
        has_account: s.has_account,
        balance_display: SharedString::from(money_br(s.balance)),
        balance_negative: s.balance < Decimal::ZERO,
        initial_display: SharedString::from(money_br(s.initial)),
        inflow_display: SharedString::from(money_br(s.inflow)),
        outflow_display: SharedString::from(money_br(s.outflow)),
        orders_display: SharedString::from(format!("+ {}", money_br(s.orders_in))),
        deposits_display: SharedString::from(format!("+ {}", money_br(s.deposits_in))),
        finance_in_display: SharedString::from(format!("+ {}", money_br(s.finance_in))),
        finance_out_display: SharedString::from(format!("− {}", money_br(s.finance_out))),
        withdrawals_display: SharedString::from(format!("− {}", money_br(s.withdrawals_out))),
        movements_count: s.movements_count,
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
        handle.spawn(async move {
            let cid = state.company_id();
            let result = state.treasury_service.open(cid, initial, notes_opt).await;
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                match result {
                    Ok(_) => {
                        notify.notify_one();
                        show_toast(&ui, "Carteira criada", "success");
                        ui.invoke_treasury_refresh();
                    }
                    Err(e) => {
                        ui.set_treasury_setup_error(SharedString::from(friendly_error(&e)));
                    }
                }
            });
        });
    });
}
