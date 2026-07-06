use std::sync::Arc;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};


use crate::context::DesktopState;
use crate::{MainWindow, WalletMovementRow, WalletSummary};

use super::core::{MovementRowRaw, refresh_for_selected, SummaryRaw};

pub(crate) fn apply_summary(ui: &MainWindow, s: SummaryRaw) {
    ui.set_wallet_summary(WalletSummary {
        has_account: s.has_account,
        account_id: SharedString::from(s.account_id),
        balance_display: SharedString::from(s.balance_display),
        balance_tone: SharedString::from(s.balance_tone),
        credit_limit_display: SharedString::from(s.credit_limit_display),
        status_label: SharedString::from(s.status_label),
        available_display: SharedString::from(s.available_display),
        movements_count: s.movements_count,
    });
}

pub(crate) fn apply_movements(ui: &MainWindow, rows: Vec<MovementRowRaw>) {
    let model: Vec<WalletMovementRow> = rows
        .into_iter()
        .map(|r| WalletMovementRow {
            id: SharedString::from(r.id),
            kind: SharedString::from(r.kind),
            title: SharedString::from(r.title),
            amount_display: SharedString::from(r.amount_display),
            amount_tone: SharedString::from(r.amount_tone),
            balance_after_display: SharedString::from(r.balance_after_display),
            time_display: SharedString::from(r.time_display),
            notes: SharedString::from(r.notes),
        })
        .collect();
    ui.set_wallet_movements(ModelRc::new(VecModel::from(model)));
}

// ── Setup callbacks ───────────────────────────────────────────────

/// Observa mudanças em `selected_customer_id` para recarregar a
/// carteira. Como Slint não tem nativo "watch property", usamos um
/// timer leve (1s). Custo desprezível e elimina race com o callback
/// `select_customer` que está em outro módulo.
pub(crate) fn setup_select_listener(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
) {
    // Tick inicial — popula vazio enquanto o user ainda não escolheu.
    refresh_for_selected(&ui.as_weak(), state, handle);
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    let last_seen = Arc::new(std::sync::Mutex::new(String::new()));
    handle.clone().spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let current = {
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
            let changed = {
                let mut g = last_seen.lock().unwrap();
                if *g != current {
                    *g = current.clone();
                    true
                } else {
                    false
                }
            };
            if changed {
                refresh_for_selected(&ui_weak, &state, &handle);
            }
        }
    });
}

pub(crate) fn setup_open_modals(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_wallet_open_deposit(move || {
        if let Some(ui) = ui_weak.upgrade() {
            reset_form(&ui);
            ui.set_wallet_show_deposit(true);
        }
    });
    let ui_weak = ui.as_weak();
    ui.on_wallet_open_withdraw(move || {
        if let Some(ui) = ui_weak.upgrade() {
            reset_form(&ui);
            ui.set_wallet_show_withdraw(true);
        }
    });
    let ui_weak = ui.as_weak();
    ui.on_wallet_open_adjust(move || {
        if let Some(ui) = ui_weak.upgrade() {
            reset_form(&ui);
            ui.set_wallet_show_adjust(true);
        }
    });
    let ui_weak = ui.as_weak();
    ui.on_wallet_open_limit(move || {
        if let Some(ui) = ui_weak.upgrade() {
            let current = ui.get_wallet_summary().credit_limit_display.to_string();
            // Limpa e pré-preenche com o limite atual sem o "R$ ".
            let pre = current
                .trim_start_matches("R$ ")
                .trim()
                .to_string();
            ui.set_wallet_form_limit(SharedString::from(pre));
            ui.set_wallet_form_notes(SharedString::from(""));
            ui.set_wallet_form_error(SharedString::from(""));
            ui.set_wallet_show_limit(true);
        }
    });
}

pub(crate) fn reset_form(ui: &MainWindow) {
    ui.set_wallet_form_amount(SharedString::from(""));
    ui.set_wallet_form_notes(SharedString::from(""));
    ui.set_wallet_form_limit(SharedString::from(""));
    ui.set_wallet_form_error(SharedString::from(""));
}

pub(crate) fn setup_close_modals(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_wallet_close_modals(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_wallet_show_deposit(false);
            ui.set_wallet_show_withdraw(false);
            ui.set_wallet_show_adjust(false);
            ui.set_wallet_show_limit(false);
        }
    });
}

