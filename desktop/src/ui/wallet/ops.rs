use std::sync::Arc;

use slint::{ComponentHandle, SharedString};
use uuid::Uuid;


use crate::context::DesktopState;
use crate::MainWindow;

use super::super::helpers::show_toast;
use super::core::refresh_for_selected;

// ── Abrir carteira ───────────────────────────────────────────────

pub(crate) fn setup_confirm_open(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<tokio::sync::Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_wallet_confirm_open(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        let customer_id_s = ui.get_selected_customer_id().to_string();
        let Ok(customer_id) = Uuid::parse_str(&customer_id_s) else { return };
        let cid = state.company_id();
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let sync_notify = sync_notify.clone();
        let handle_inner = handle.clone();
        handle.spawn(async move {
            match state.wallet_service.open_account(cid, customer_id, rust_decimal::Decimal::ZERO).await {
                Ok(_) => {
                    sync_notify.notify_one();
                    let ui_weak_toast = ui_weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak_toast.upgrade() {
                            show_toast(&ui, "Carteira aberta", "success");
                        }
                    });
                    refresh_for_selected(&ui_weak, &state, &handle_inner);
                }
                Err(e) => toast_err(&ui_weak, &e.to_string()),
            }
        });
    });
}

// ── Operações ─────────────────────────────────────────────────────

pub(crate) fn setup_confirm_deposit(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<tokio::sync::Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_wallet_confirm_deposit(move || {
        confirm_op(
            &ui_weak,
            &state,
            &handle,
            sync_notify.clone(),
            OpKind::Deposit,
        );
    });
}

pub(crate) fn setup_confirm_withdraw(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<tokio::sync::Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_wallet_confirm_withdraw(move || {
        confirm_op(
            &ui_weak,
            &state,
            &handle,
            sync_notify.clone(),
            OpKind::Withdraw,
        );
    });
}

pub(crate) fn setup_confirm_adjust(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<tokio::sync::Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_wallet_confirm_adjust(move || {
        confirm_op(
            &ui_weak,
            &state,
            &handle,
            sync_notify.clone(),
            OpKind::Adjust,
        );
    });
}

pub(crate) fn setup_confirm_limit(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<tokio::sync::Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_wallet_confirm_limit(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        let raw = ui.get_wallet_form_limit().to_string();
        let Some(limit) = parse_amount(&raw, false) else {
            ui.set_wallet_form_error(SharedString::from("Informe um valor válido"));
            return;
        };
        if limit < 0.0 {
            ui.set_wallet_form_error(SharedString::from("Limite deve ser zero ou positivo"));
            return;
        }
        let account_id_s = ui.get_wallet_summary().account_id.to_string();
        let Ok(account_id) = Uuid::parse_str(&account_id_s) else {
            ui.set_wallet_form_error(SharedString::from("Abra a carteira primeiro"));
            return;
        };
        let cid = state.company_id();
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let sync_notify = sync_notify.clone();
        let handle_inner = handle.clone();
        handle.spawn(async move {
            match state
                .wallet_service
                .set_credit_limit(cid, account_id, letaf_core::money::from_db_f64(limit))
                .await
            {
                Ok(_) => {
                    sync_notify.notify_one();
                    let ui_weak2 = ui_weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak2.upgrade() {
                            ui.set_wallet_show_limit(false);
                            show_toast(&ui, "Limite atualizado", "success");
                        }
                    });
                    refresh_for_selected(&ui_weak, &state, &handle_inner);
                }
                Err(e) => set_form_error(&ui_weak, &e.to_string()),
            }
        });
    });
}

#[derive(Clone, Copy)]
pub(crate) enum OpKind {
    Deposit,
    Withdraw,
    Adjust,
}

pub(crate) fn confirm_op(
    ui_weak: &slint::Weak<MainWindow>,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<tokio::sync::Notify>,
    op: OpKind,
) {
    let Some(ui) = ui_weak.upgrade() else { return };
    let amount_s = ui.get_wallet_form_amount().to_string();
    let notes_s = ui.get_wallet_form_notes().to_string();
    // ajuste permite negativo
    let allow_neg = matches!(op, OpKind::Adjust);
    let Some(amount) = parse_amount(&amount_s, allow_neg) else {
        ui.set_wallet_form_error(SharedString::from(
            "Informe um valor válido",
        ));
        return;
    };
    if !allow_neg && amount <= 0.0 {
        ui.set_wallet_form_error(SharedString::from(
            "Valor deve ser maior que zero",
        ));
        return;
    }
    if matches!(op, OpKind::Adjust) && notes_s.trim().is_empty() {
        ui.set_wallet_form_error(SharedString::from(
            "Justificativa obrigatória para ajuste manual",
        ));
        return;
    }
    let account_id_s = ui.get_wallet_summary().account_id.to_string();
    let Ok(account_id) = Uuid::parse_str(&account_id_s) else {
        ui.set_wallet_form_error(SharedString::from("Abra a carteira primeiro"));
        return;
    };
    let cid = state.company_id();
    let ui_weak = ui_weak.clone();
    let state = state.clone();
    let sync_notify = sync_notify.clone();
    let handle_inner = handle.clone();
    let notes_opt = if notes_s.is_empty() { None } else { Some(notes_s.clone()) };
    handle.spawn(async move {
        let result = match op {
            OpKind::Deposit => state
                .wallet_service
                .deposit(cid, account_id, letaf_core::money::from_db_f64(amount), notes_opt)
                .await
                .map(|_| ()),
            OpKind::Withdraw => state
                .wallet_service
                .withdraw(cid, account_id, letaf_core::money::from_db_f64(amount), notes_opt)
                .await
                .map(|_| ()),
            OpKind::Adjust => state
                .wallet_service
                .manual_adjust(cid, account_id, letaf_core::money::from_db_f64(amount), notes_s)
                .await
                .map(|_| ()),
        };
        match result {
            Ok(()) => {
                sync_notify.notify_one();
                let ui_weak2 = ui_weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak2.upgrade() {
                        ui.set_wallet_show_deposit(false);
                        ui.set_wallet_show_withdraw(false);
                        ui.set_wallet_show_adjust(false);
                        show_toast(&ui, "Operação registrada", "success");
                    }
                });
                refresh_for_selected(&ui_weak, &state, &handle_inner);
            }
            Err(e) => set_form_error(&ui_weak, &e.to_string()),
        }
    });
}

pub(crate) fn set_form_error(ui_weak: &slint::Weak<MainWindow>, msg: &str) {
    let msg = msg.to_string();
    let ui_weak = ui_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_wallet_form_error(SharedString::from(msg));
        }
    });
}

pub(crate) fn toast_err(ui_weak: &slint::Weak<MainWindow>, msg: &str) {
    let msg = msg.to_string();
    let ui_weak = ui_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_weak.upgrade() {
            show_toast(&ui, &msg, "error");
        }
    });
}

pub(crate) fn parse_amount(s: &str, allow_neg: bool) -> Option<f64> {
    let cleaned = s
        .trim()
        .replace("R$", "")
        .replace([' ', '.'], "")
        .replace(',', ".");
    let v = cleaned.parse::<f64>().ok()?;
    if !allow_neg && v < 0.0 {
        return None;
    }
    Some(v)
}

// ── Sync listener ────────────────────────────────────────────────

pub(crate) fn setup_sync_listener(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    cycle_done: Arc<tokio::sync::Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle_inner = handle.clone();
    handle.spawn(async move {
        loop {
            cycle_done.notified().await;
            let visible = {
                let (tx, rx) = std::sync::mpsc::channel();
                let ui_weak2 = ui_weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    let active = ui_weak2
                        .upgrade()
                        .map(|u| u.get_active_tab().to_string())
                        .unwrap_or_default();
                    let _ = tx.send(active == "customers");
                });
                rx.recv().unwrap_or(false)
            };
            if !visible {
                continue;
            }
            refresh_for_selected(&ui_weak, &state, &handle_inner);
        }
    });
}
