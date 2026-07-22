use std::sync::Arc;
use rust_decimal::prelude::ToPrimitive;

use chrono::Local;
use uuid::Uuid;

use letaf_core::wallet::model::{WalletAccount, WalletMovement, WalletMovementKind};

use crate::context::DesktopState;
use crate::format::money_br;
use crate::MainWindow;

use super::ops::{setup_confirm_adjust, setup_confirm_deposit, setup_confirm_limit, setup_confirm_open, setup_confirm_withdraw, setup_sync_listener};
use super::view::{apply_movements, apply_summary, setup_close_modals, setup_open_modals, setup_select_listener};

pub(crate) fn setup_wallet(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<tokio::sync::Notify>,
    sync_cycle_done: tokio::sync::watch::Receiver<u64>,
) {
    setup_select_listener(ui, state, handle);
    setup_open_modals(ui);
    setup_close_modals(ui);
    setup_confirm_open(ui, state, handle, sync_notify.clone());
    setup_confirm_deposit(ui, state, handle, sync_notify.clone());
    setup_confirm_withdraw(ui, state, handle, sync_notify.clone());
    setup_confirm_adjust(ui, state, handle, sync_notify.clone());
    setup_confirm_limit(ui, state, handle, sync_notify);
    setup_sync_listener(ui, state, handle, sync_cycle_done);
}

// ── Listener da seleção de cliente ──────────────────────────────
// O `select_customer` já existe na tela de Clientes; aqui só
// re-popula a carteira sempre que `selected_customer_id` muda.
// Para isso o setup principal expõe um helper que pode ser chamado
// pelo módulo `customers` após cada select.

/// Recarrega a carteira do cliente atualmente selecionado e envia
/// para a UI. Pode ser chamado de fora (ex.: customers.rs após
/// select_customer) ou daqui após uma operação.
pub(crate) fn refresh_for_selected(
    ui_weak: &slint::Weak<MainWindow>,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
) {
    let ui_weak = ui_weak.clone();
    let state = state.clone();
    handle.spawn(async move {
        let selected_id = {
            let (tx, rx) = std::sync::mpsc::channel();
            let ui_weak2 = ui_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                let id = ui_weak2
                    .upgrade()
                    .map(|u| u.get_selected_customer_id().to_string())
                    .unwrap_or_default();
                let _ = tx.send(id);
            });
            rx.recv().unwrap_or_default()
        };
        let summary = build_summary(&state, &selected_id).await;
        let movements = if summary.has_account {
            load_movements_raw(
                &state,
                Uuid::parse_str(&summary.account_id).unwrap_or_else(|_| Uuid::nil()),
            )
            .await
        } else {
            Vec::new()
        };
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_weak.upgrade() {
                apply_summary(&ui, summary);
                apply_movements(&ui, movements);
            }
        });
    });
}

// Snapshot enviado pra UI — Send-safe.
pub(crate) struct SummaryRaw {
    pub(crate) has_account: bool,
    pub(crate) account_id: String,
    pub(crate) balance_display: String,
    pub(crate) balance_tone: String,
    pub(crate) credit_limit_display: String,
    pub(crate) status_label: String,
    pub(crate) available_display: String,
    pub(crate) movements_count: i32,
}

#[derive(Clone)]
pub(crate) struct MovementRowRaw {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) title: String,
    pub(crate) amount_display: String,
    pub(crate) amount_tone: String,
    pub(crate) balance_after_display: String,
    pub(crate) time_display: String,
    pub(crate) notes: String,
}

pub(crate) async fn build_summary(state: &DesktopState, customer_id_s: &str) -> SummaryRaw {
    if customer_id_s.is_empty() {
        return SummaryRaw {
            has_account: false,
            account_id: String::new(),
            balance_display: money_br(rust_decimal::Decimal::ZERO),
            balance_tone: "neutral".into(),
            credit_limit_display: money_br(rust_decimal::Decimal::ZERO),
            status_label: String::new(),
            available_display: String::new(),
            movements_count: 0,
        };
    }
    let Ok(customer_id) = Uuid::parse_str(customer_id_s) else {
        return empty_summary();
    };
    let cid = state.company_id();
    let account = state
        .wallet_service
        .find_account_by_customer(cid, customer_id)
        .await
        .ok()
        .flatten();
    let Some(account) = account else {
        return empty_summary();
    };
    let movements_count = state
        .wallet_service
        .find_movements(cid, account.base.id, 9_999)
        .await
        .map(|v| v.len() as i32)
        .unwrap_or(0);
    SummaryRaw {
        has_account: true,
        account_id: account.base.id.to_string(),
        balance_display: money_signed(account.balance),
        balance_tone: tone(account.balance),
        credit_limit_display: money_br(account.credit_limit),
        status_label: status_label(&account),
        available_display: format!(
            "Saldo disponível: {}",
            money_br(account.balance + account.credit_limit)
        ),
        movements_count,
    }
}

pub(crate) fn empty_summary() -> SummaryRaw {
    SummaryRaw {
        has_account: false,
        account_id: String::new(),
        balance_display: money_br(rust_decimal::Decimal::ZERO),
        balance_tone: "neutral".into(),
        credit_limit_display: money_br(rust_decimal::Decimal::ZERO),
        status_label: String::new(),
        available_display: String::new(),
        movements_count: 0,
    }
}

pub(crate) fn status_label(a: &WalletAccount) -> String {
    if a.balance >= rust_decimal::Decimal::ZERO {
        if a.credit_limit > rust_decimal::Decimal::ZERO {
            format!("Em Dia · Limite {}", money_br(a.credit_limit))
        } else {
            "Em Dia · Sem fiado configurado".into()
        }
    } else {
        let used = ((-a.balance / a.credit_limit.max(rust_decimal::Decimal::new(1, 3))).to_f64().unwrap_or(0.0) * 100.0).round() as i64;
        format!("Em Fiado · Usando {}% do limite", used.clamp(0, 999))
    }
}

pub(crate) async fn load_movements_raw(state: &DesktopState, account_id: Uuid) -> Vec<MovementRowRaw> {
    if account_id.is_nil() {
        return Vec::new();
    }
    let cid = state.company_id();
    let movements = state
        .wallet_service
        .find_movements(cid, account_id, 50)
        .await
        .unwrap_or_default();
    movements
        .into_iter()
        .map(|m| MovementRowRaw {
            id: m.base.id.to_string(),
            kind: m.kind.to_string(),
            title: movement_title(&m),
            amount_display: format_amount(&m),
            amount_tone: tone_of_movement(&m),
            balance_after_display: format!("Saldo: {}", money_signed(m.balance_after)),
            time_display: format_time(&m),
            notes: m.notes.clone().unwrap_or_default(),
        })
        .collect()
}

pub(crate) fn movement_title(m: &WalletMovement) -> String {
    match m.kind {
        WalletMovementKind::Deposit => "Depósito".into(),
        WalletMovementKind::Withdraw => "Saque".into(),
        WalletMovementKind::OrderCharge => "Cobrança em pedido".into(),
        WalletMovementKind::OrderRefund => "Estorno de pedido".into(),
        WalletMovementKind::ManualAdjust => "Ajuste Manual".into(),
    }
}

pub(crate) fn format_amount(m: &WalletMovement) -> String {
    let signed = m.amount * m.kind.sign();
    money_signed(signed)
}

pub(crate) fn tone_of_movement(m: &WalletMovement) -> String {
    let signed = m.amount * m.kind.sign();
    if signed >= rust_decimal::Decimal::ZERO { "pos".into() } else { "neg".into() }
}

pub(crate) fn format_time(m: &WalletMovement) -> String {
    // UTC → Local pra exibição.
    let utc = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
        m.base.created_at,
        chrono::Utc,
    );
    utc.with_timezone(&Local).format("%d/%m/%Y · %H:%M").to_string()
}

pub(crate) fn money_signed(v: rust_decimal::Decimal) -> String {
    if v >= rust_decimal::Decimal::ZERO {
        money_br(v)
    } else {
        format!("R$ -{}", money_br(-v).trim_start_matches("R$ "))
    }
}

pub(crate) fn tone(v: rust_decimal::Decimal) -> String {
    if v < rust_decimal::Decimal::new(-5, 3) {
        "neg".into()
    } else if v > rust_decimal::Decimal::new(5, 3) {
        "pos".into()
    } else {
        "neutral".into()
    }
}

