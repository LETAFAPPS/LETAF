use std::sync::Arc;

use slint::{ComponentHandle, SharedString};
use uuid::Uuid;


use crate::context::DesktopState;
use crate::MainWindow;

use super::super::helpers::{show_toast, user_error};
use crate::format::{money_br as fmt_brl, money_br_signed as fmt_brl_signed};
use super::core::parse_amount;

// ── Abrir caixa ──────────────────────────────────────────────────

pub(crate) fn setup_open_confirm(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<tokio::sync::Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_cash_open_confirm(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        let amount = parse_amount(&ui_ref.get_cash_open_amount());
        let notes = ui_ref.get_cash_open_notes().to_string();
        let notes_opt = if notes.trim().is_empty() {
            None
        } else {
            Some(notes)
        };
        // Sem user_id persistido localmente — usamos rótulo da role
        // como nome (snapshot) e Uuid::nil() como operator_id. O nome
        // real do operador será carregado do servidor no pull.
        let role_label = ui_ref.get_user_role().to_string();
        let operator_name = if role_label.trim().is_empty() {
            "Operador".to_string()
        } else {
            role_label
        };
        let operator_id = Uuid::nil();

        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let notify = sync_notify.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            let result = state
                .cash_service
                .open_session(cid, operator_id, operator_name, amount, notes_opt)
                .await;
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                match result {
                    Ok(_) => {
                        ui.set_cash_show_open(false);
                        ui.set_cash_open_amount(SharedString::default());
                        ui.set_cash_open_notes(SharedString::default());
                        show_toast(&ui, "Caixa Aberto", "success");
                        ui.invoke_cash_refresh();
                        notify.notify_one();
                    }
                    Err(e) => {
                        show_toast(&ui, &user_error(&e), "error");
                    }
                }
            });
        });
    });
}

// ── Sangria ──────────────────────────────────────────────────────

pub(crate) fn setup_sangria_confirm(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<tokio::sync::Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_cash_sangria_confirm(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        let amount = parse_amount(&ui_ref.get_cash_sangria_amount());
        let reason = ui_ref.get_cash_sangria_reason().to_string();
        let detail = ui_ref.get_cash_sangria_detail().to_string();
        let detail_opt = if detail.trim().is_empty() {
            None
        } else {
            Some(detail)
        };
        let session_id_str = ui_ref.get_cash_summary().session_id.to_string();
        let Ok(session_id) = Uuid::parse_str(&session_id_str) else {
            show_toast(&ui_ref, "Nenhuma sessão aberta", "error");
            return;
        };

        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let notify = sync_notify.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            let result = state
                .cash_service
                .register_sangria(cid, session_id, amount, reason, detail_opt)
                .await;
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                match result {
                    Ok(_) => {
                        ui.set_cash_show_sangria(false);
                        ui.set_cash_sangria_amount(SharedString::default());
                        ui.set_cash_sangria_detail(SharedString::default());
                        show_toast(&ui, "Sangria registrada", "success");
                        ui.invoke_cash_refresh();
                        notify.notify_one();
                    }
                    Err(e) => {
                        show_toast(&ui, &user_error(&e), "error");
                    }
                }
            });
        });
    });
}

// ── Suprimento ───────────────────────────────────────────────────

pub(crate) fn setup_suprimento_confirm(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<tokio::sync::Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_cash_suprimento_confirm(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        let amount = parse_amount(&ui_ref.get_cash_suprimento_amount());
        let origin = ui_ref.get_cash_suprimento_origin().to_string();
        let detail = ui_ref.get_cash_suprimento_detail().to_string();
        let detail_opt = if detail.trim().is_empty() {
            None
        } else {
            Some(detail)
        };
        let session_id_str = ui_ref.get_cash_summary().session_id.to_string();
        let Ok(session_id) = Uuid::parse_str(&session_id_str) else {
            show_toast(&ui_ref, "Nenhuma sessão aberta", "error");
            return;
        };

        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let notify = sync_notify.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            let result = state
                .cash_service
                .register_suprimento(cid, session_id, amount, origin, detail_opt)
                .await;
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                match result {
                    Ok(_) => {
                        ui.set_cash_show_suprimento(false);
                        ui.set_cash_suprimento_amount(SharedString::default());
                        ui.set_cash_suprimento_detail(SharedString::default());
                        show_toast(&ui, "Suprimento registrado", "success");
                        ui.invoke_cash_refresh();
                        notify.notify_one();
                    }
                    Err(e) => {
                        show_toast(&ui, &user_error(&e), "error");
                    }
                }
            });
        });
    });
}

// ── Fechar caixa: recalc (preview) e confirm ─────────────────────

pub(crate) fn setup_close_recalc(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_cash_close_recalc(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        // Sistema (vem do summary cacheado) + informado (input do usuário).
        let summary = ui.get_cash_summary();
        // Header info: opened-at, duration, operator.
        ui.set_cash_close_opened_at(summary.opened_summary.clone());

        let sys_cash = parse_brl(&summary.cash_now_display);
        let sys_total_expected = parse_brl(&summary.total_expected_display);

        // Pra preservar simplicidade, lemos sistema dos campos sys-* já
        // populados (set no refresh). Mas recalc usa o que tá na UI.
        let sys_cash_ui = parse_brl(&ui.get_cash_close_sys_cash());
        let sys_pix = parse_brl(&ui.get_cash_close_sys_pix());
        let sys_credit = parse_brl(&ui.get_cash_close_sys_credit());
        let sys_debit = parse_brl(&ui.get_cash_close_sys_debit());

        let sys_cash_used = if sys_cash_ui > 0.0 { sys_cash_ui } else { sys_cash };
        let _ = sys_total_expected;

        let in_cash = parse_amount(&ui.get_cash_close_in_cash());
        let in_pix = parse_amount(&ui.get_cash_close_in_pix());
        let in_credit = parse_amount(&ui.get_cash_close_in_credit());
        let in_debit = parse_amount(&ui.get_cash_close_in_debit());

        let diff_cash = in_cash - sys_cash_used;
        let diff_pix = in_pix - sys_pix;
        let diff_credit = in_credit - sys_credit;
        let diff_debit = in_debit - sys_debit;
        let sys_total = sys_cash_used + sys_pix + sys_credit + sys_debit;
        let in_total = in_cash + in_pix + in_credit + in_debit;
        let diff_total = in_total - sys_total;

        ui.set_cash_close_sys_total(SharedString::from(fmt_brl(sys_total)));
        ui.set_cash_close_in_total(SharedString::from(fmt_brl(in_total)));
        ui.set_cash_close_diff_cash(SharedString::from(fmt_brl_signed(diff_cash)));
        ui.set_cash_close_diff_pix(SharedString::from(fmt_brl_signed(diff_pix)));
        ui.set_cash_close_diff_credit(SharedString::from(fmt_brl_signed(diff_credit)));
        ui.set_cash_close_diff_debit(SharedString::from(fmt_brl_signed(diff_debit)));
        ui.set_cash_close_diff_total(SharedString::from(fmt_brl_signed(diff_total)));
        ui.set_cash_close_has_diff(diff_cash.abs() > 0.005);
    });
}

pub(crate) fn setup_close_confirm(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<tokio::sync::Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_cash_close_confirm(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        let session_id_str = ui_ref.get_cash_summary().session_id.to_string();
        let Ok(session_id) = Uuid::parse_str(&session_id_str) else {
            show_toast(&ui_ref, "Nenhuma sessão aberta", "error");
            return;
        };
        let counted = parse_amount(&ui_ref.get_cash_close_in_cash());
        let notes = ui_ref.get_cash_close_notes().to_string();
        let has_diff = ui_ref.get_cash_close_has_diff();
        if has_diff && notes.trim().is_empty() {
            show_toast(&ui_ref, "Observação é obrigatória quando há diferença", "error");
            return;
        }
        let notes_opt = if notes.trim().is_empty() {
            None
        } else {
            Some(notes)
        };

        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let notify = sync_notify.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            let result = state
                .cash_service
                .close_session(cid, session_id, counted, notes_opt)
                .await;
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                match result {
                    Ok(_) => {
                        ui.set_cash_show_close(false);
                        ui.set_cash_close_in_cash(SharedString::default());
                        ui.set_cash_close_in_pix(SharedString::default());
                        ui.set_cash_close_in_credit(SharedString::default());
                        ui.set_cash_close_in_debit(SharedString::default());
                        ui.set_cash_close_notes(SharedString::default());
                        show_toast(&ui, "Caixa Fechado", "success");
                        ui.invoke_cash_refresh();
                        notify.notify_one();
                    }
                    Err(e) => {
                        show_toast(&ui, &user_error(&e), "error");
                    }
                }
            });
        });
    });
}

/// Parsea "R$ 1.234,56" → 1234.56. Robusto a sinais e moeda.
pub(crate) fn parse_brl(s: &str) -> f64 {
    let cleaned: String = s
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == ',' || *c == '.' || *c == '-')
        .collect();
    // pt-BR: remove '.' (milhar), troca ',' por '.'
    let normalized = cleaned.replace('.', "").replace(',', ".");
    normalized.parse::<f64>().unwrap_or(0.0)
}
