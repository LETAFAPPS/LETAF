//! Tela "Carteira" — tesouraria do estabelecimento.
//!
//! Regras (AI_RULES.md §1, §3, §14): a UI SÓ RENDERIZA. A consolidação
//! (o que é dinheiro novo entrando/saindo, saldo derivado, recorte do
//! dia e do mês) vive em `letaf_core::treasury::analytics` — aqui só
//! ficam rótulos, cores e geometria.
//!
//! O SALDO considera todo o histórico; entradas/saídas mostram o DIA
//! corrente (no fuso da loja), o mini-gráfico mostra as ÚLTIMAS 12
//! HORAS e a lista traz as 15 últimas movimentações.

use std::sync::Arc;

use chrono::{Duration, NaiveDateTime};
use rust_decimal::Decimal;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use letaf_core::treasury::analytics::{
    self, CashDetail, CashMovement, CashSources, DayBreakdown, HourFlow, TreasurySnapshot,
};
use letaf_core::wallet::model::WalletMovement;

use crate::context::DesktopState;
use crate::format::{format_order_date, format_order_time, money_br};
use crate::{
    MainWindow, TreasuryBreakdownRow, TreasuryChartBar, TreasuryMovementRow, TreasurySummary,
};

use super::helpers::{friendly_error, show_toast};

/// Quantas movimentações a lista mostra (o rótulo "Últimas N" do card
/// em `treasury_page.slint` acompanha este número).
const MOVEMENTS_LIMIT: usize = 15;

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
    /// Detalhamento do dia por origem (entradas e saídas).
    breakdown: Vec<BreakdownRaw>,
    movements_count: i32,
    goal: Decimal,
    reserved: Decimal,
}

/// Uma origem do detalhamento (já somada no dia).
struct BreakdownRaw {
    label: &'static str,
    amount: Decimal,
    negative: bool,
}

/// Linha da lista de movimentações, já com os textos em pt-BR.
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

async fn build_snapshot(
    state: &DesktopState,
) -> (SummaryRaw, Vec<MovementRaw>, Vec<TreasuryChartBar>) {
    let cid = state.company_id();
    let Some(treasury) = state.treasury_service.find(cid).await.ok().flatten() else {
        return (empty_summary(), Vec::new(), Vec::new());
    };

    // Carrega as fontes (§10: acesso a dados via service/repository) e
    // entrega ao core, que decide o que é dinheiro novo.
    let orders = state.order_service.find_all(cid).await.unwrap_or_default();
    let wallet_movements = load_wallet_movements(state).await;
    let finance_entries = state.finance_service.find_all(cid).await.unwrap_or_default();
    let cashbox_movements = state
        .treasury_service
        .find_movements(cid, 9_999)
        .await
        .unwrap_or_default();

    let snapshot = analytics::consolidate(
        &CashSources {
            treasury: &treasury,
            orders: &orders,
            wallet_movements: &wallet_movements,
            finance_entries: &finance_entries,
            cashbox_movements: &cashbox_movements,
        },
        letaf_core::tz::today(),
    );

    let chart = build_chart(&snapshot.movements);
    let rows = snapshot
        .movements
        .iter()
        .take(MOVEMENTS_LIMIT)
        .map(movement_row)
        .collect();
    (summary_of(&snapshot), rows, chart)
}

/// Movimentos de TODAS as carteiras de clientes numa lista só — o core
/// classifica quais representam dinheiro entrando ou saindo do caixa.
async fn load_wallet_movements(state: &DesktopState) -> Vec<WalletMovement> {
    let cid = state.company_id();
    let accounts = state
        .wallet_service
        .find_all_accounts(cid)
        .await
        .unwrap_or_default();
    let mut all = Vec::new();
    for account in &accounts {
        let list = state
            .wallet_service
            .find_movements(cid, account.base.id, 9_999)
            .await
            .unwrap_or_default();
        all.extend(list);
    }
    all
}

/// Retrato do core → resumo do card.
fn summary_of(s: &TreasurySnapshot) -> SummaryRaw {
    SummaryRaw {
        has_account: true,
        balance: s.balance,
        initial: s.initial,
        inflow: s.inflow,
        outflow: s.outflow,
        breakdown: breakdown_rows(&s.breakdown),
        movements_count: s.movements.len() as i32,
        goal: s.goal,
        reserved: s.reserved,
    }
}

/// Rótulos pt-BR de cada origem do detalhamento do dia.
fn breakdown_rows(b: &DayBreakdown) -> Vec<BreakdownRaw> {
    vec![
        BreakdownRaw { label: "Pedidos pagos", amount: b.orders_in, negative: false },
        BreakdownRaw {
            label: "Depósitos na carteira de clientes",
            amount: b.wallet_in,
            negative: false,
        },
        BreakdownRaw {
            label: "Financeiro · recebimentos",
            amount: b.finance_in,
            negative: false,
        },
        BreakdownRaw { label: "Aportes no caixa", amount: b.cashbox_in, negative: false },
        BreakdownRaw {
            label: "Financeiro · pagamentos",
            amount: b.finance_out,
            negative: true,
        },
        BreakdownRaw {
            label: "Saques da carteira de clientes",
            amount: b.wallet_out,
            negative: true,
        },
        BreakdownRaw { label: "Retiradas do caixa", amount: b.cashbox_out, negative: true },
    ]
}

/// Uma linha da lista: usa o texto do próprio movimento (observação do
/// operador, descrição do lançamento) e cai no rótulo padrão da origem.
fn movement_row(m: &CashMovement) -> MovementRaw {
    let (source, default_title) = movement_labels(m.detail);
    MovementRaw {
        title: m.description.clone().unwrap_or(default_title),
        source: source.to_string(),
        at: m.at,
        amount: m.amount,
        positive: m.positive,
    }
}

/// (origem, título padrão) de cada tipo de movimento.
fn movement_labels(detail: CashDetail) -> (&'static str, String) {
    match detail {
        CashDetail::OrderPayment { number } => ("Pedido", format!("Pedido #{:04}", number)),
        CashDetail::WalletDeposit => ("Carteira", "Depósito de cliente".into()),
        CashDetail::WalletWithdraw => ("Carteira", "Saque de cliente".into()),
        CashDetail::WalletAdjust => ("Carteira", "Ajuste de carteira".into()),
        // Lançamento do Financeiro sempre tem descrição própria.
        CashDetail::FinanceReceived | CashDetail::FinancePaid => ("Financeiro", String::new()),
        CashDetail::CashboxDeposit => ("Caixa", "Depósito no caixa".into()),
        CashDetail::CashboxWithdraw => ("Caixa", "Retirada do caixa".into()),
    }
}

fn empty_summary() -> SummaryRaw {
    SummaryRaw {
        has_account: false,
        balance: Decimal::ZERO,
        initial: Decimal::ZERO,
        inflow: Decimal::ZERO,
        outflow: Decimal::ZERO,
        breakdown: Vec::new(),
        movements_count: 0,
        goal: Decimal::ZERO,
        reserved: Decimal::ZERO,
    }
}

/// Mini-gráfico do card de saldo: as ÚLTIMAS 12 HORAS, uma barra por
/// hora cheia, terminando na hora ATUAL (a última barra, destacada).
/// Cada barra é o líquido da hora (entradas − saídas): positivo sobe,
/// negativo desce. A altura é relativa à maior movimentação da janela.
/// Entradas, saídas e líquido já vão formatados para o card que a UI
/// mostra ao passar o mouse (§1/§14).
fn build_chart(movements: &[CashMovement]) -> Vec<TreasuryChartBar> {
    const SLOTS: usize = 12;
    use rust_decimal::prelude::ToPrimitive;

    // A janela de horas é contada no relógio da loja (o core recebe a
    // hora cheia atual e faz o recorte).
    let flows = analytics::hourly_flow(movements, letaf_core::tz::current_hour(), SLOTS);
    let max = flows
        .iter()
        .map(|f| f.net().abs().to_f64().unwrap_or(0.0))
        .fold(0.0_f64, f64::max);
    flows
        .iter()
        .enumerate()
        .map(|(i, f)| chart_bar(f, max, i + 1 == SLOTS))
        .collect()
}

/// Uma barra da série horária, já formatada.
fn chart_bar(f: &HourFlow, max: f64, current: bool) -> TreasuryChartBar {
    use rust_decimal::prelude::ToPrimitive;
    let net = f.net();
    TreasuryChartBar {
        progress: if max > 0.0 {
            (net.abs().to_f64().unwrap_or(0.0) / max) as f32
        } else {
            0.0
        },
        positive: net >= Decimal::ZERO,
        current,
        hour_label: SharedString::from(format!(
            "{} – {}",
            f.start.format("%H:00"),
            (f.start + Duration::hours(1)).format("%H:00")
        )),
        inflow_display: SharedString::from(format!("+ {}", money_br(f.inflow))),
        outflow_display: SharedString::from(format!("− {}", money_br(f.outflow))),
        net_display: SharedString::from(if net < Decimal::ZERO {
            format!("− {}", money_br(-net))
        } else {
            format!("+ {}", money_br(net))
        }),
    }
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

    ui.set_treasury_breakdown_in(breakdown_model(&s.breakdown, false));
    ui.set_treasury_breakdown_out(breakdown_model(&s.breakdown, true));
    ui.set_treasury_chart(ModelRc::new(VecModel::from(chart.to_vec())));
}

/// Monta o modelo de um dos grupos do detalhamento (um card por
/// origem). Origens sem movimento no dia ficam apagadas.
fn breakdown_model(rows: &[BreakdownRaw], negative: bool) -> ModelRc<TreasuryBreakdownRow> {
    let sign = if negative { "−" } else { "+" };
    let out: Vec<TreasuryBreakdownRow> = rows
        .iter()
        .filter(|r| r.negative == negative)
        .map(|r| TreasuryBreakdownRow {
            label: SharedString::from(r.label),
            value: SharedString::from(format!("{sign} {}", money_br(r.amount))),
            negative,
            muted: r.amount <= Decimal::ZERO,
        })
        .collect();
    ModelRc::new(VecModel::from(out))
}

// ── Cadastro inicial ────────────────────────────────────────────

/// Parse de valor monetário pt-BR — fonte única em `crate::format`.
fn parse_money_br(s: &str) -> Option<Decimal> {
    crate::format::parse_money_br(s)
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
