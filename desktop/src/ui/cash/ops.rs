use std::sync::Arc;

use slint::{ComponentHandle, SharedString};
use uuid::Uuid;


use crate::context::DesktopState;
use crate::MainWindow;

use super::super::helpers::{show_toast, user_error};
/// Wrappers de exibição para os valores de FECHAMENTO de caixa, que são
/// contados na UI em `f64` (contagem manual). Convertem para `Decimal`
/// (round2) antes de formatar — dinheiro exato só é exigido nos cálculos
/// do domínio; aqui é apresentação da conferência.
fn fmt_brl(v: f64) -> String { crate::format::money_br_f64(v) }
fn fmt_brl_signed(v: f64) -> String { crate::format::money_br_signed(letaf_core::money::from_db_f64(v)) }
use super::core::parse_amount;
use crate::CashState;

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
    ui.global::<CashState>().on_cash_open_confirm(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        let amount = parse_amount(&ui_ref.global::<CashState>().get_cash_open_amount());
        let notes = ui_ref.global::<CashState>().get_cash_open_notes().to_string();
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
                .open_session(cid, operator_id, operator_name, letaf_core::money::from_db_f64(amount), notes_opt)
                .await;
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                match result {
                    Ok(_) => {
                        ui.global::<CashState>().set_cash_show_open(false);
                        ui.global::<CashState>().set_cash_open_amount(SharedString::default());
                        ui.global::<CashState>().set_cash_open_notes(SharedString::default());
                        show_toast(&ui, "Caixa Aberto", "success");
                        ui.global::<CashState>().invoke_cash_refresh();
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
    ui.global::<CashState>().on_cash_sangria_confirm(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        let amount = parse_amount(&ui_ref.global::<CashState>().get_cash_sangria_amount());
        let reason = ui_ref.global::<CashState>().get_cash_sangria_reason().to_string();
        let detail = ui_ref.global::<CashState>().get_cash_sangria_detail().to_string();
        let detail_opt = if detail.trim().is_empty() {
            None
        } else {
            Some(detail)
        };
        let session_id_str = ui_ref.global::<CashState>().get_cash_summary().session_id.to_string();
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
                .register_sangria(cid, session_id, letaf_core::money::from_db_f64(amount), reason, detail_opt)
                .await;
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                match result {
                    Ok(_) => {
                        ui.global::<CashState>().set_cash_show_sangria(false);
                        ui.global::<CashState>().set_cash_sangria_amount(SharedString::default());
                        ui.global::<CashState>().set_cash_sangria_detail(SharedString::default());
                        show_toast(&ui, "Sangria registrada", "success");
                        ui.global::<CashState>().invoke_cash_refresh();
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
    ui.global::<CashState>().on_cash_suprimento_confirm(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        let amount = parse_amount(&ui_ref.global::<CashState>().get_cash_suprimento_amount());
        let origin = ui_ref.global::<CashState>().get_cash_suprimento_origin().to_string();
        let detail = ui_ref.global::<CashState>().get_cash_suprimento_detail().to_string();
        let detail_opt = if detail.trim().is_empty() {
            None
        } else {
            Some(detail)
        };
        let session_id_str = ui_ref.global::<CashState>().get_cash_summary().session_id.to_string();
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
                .register_suprimento(cid, session_id, letaf_core::money::from_db_f64(amount), origin, detail_opt)
                .await;
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                match result {
                    Ok(_) => {
                        ui.global::<CashState>().set_cash_show_suprimento(false);
                        ui.global::<CashState>().set_cash_suprimento_amount(SharedString::default());
                        ui.global::<CashState>().set_cash_suprimento_detail(SharedString::default());
                        show_toast(&ui, "Suprimento registrado", "success");
                        ui.global::<CashState>().invoke_cash_refresh();
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
    ui.global::<CashState>().on_cash_close_recalc(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        // Sistema (vem do summary cacheado) + informado (input do usuário).
        let summary = ui.global::<CashState>().get_cash_summary();
        // Header info: opened-at, duration, operator.
        ui.global::<CashState>().set_cash_close_opened_at(summary.opened_summary.clone());

        let sys_cash = parse_brl(&summary.cash_now_display);
        let sys_total_expected = parse_brl(&summary.total_expected_display);

        // Pra preservar simplicidade, lemos sistema dos campos sys-* já
        // populados (set no refresh). Mas recalc usa o que tá na UI.
        let sys_cash_ui = parse_brl(&ui.global::<CashState>().get_cash_close_sys_cash());
        let sys_pix = parse_brl(&ui.global::<CashState>().get_cash_close_sys_pix());
        let sys_credit = parse_brl(&ui.global::<CashState>().get_cash_close_sys_credit());
        let sys_debit = parse_brl(&ui.global::<CashState>().get_cash_close_sys_debit());

        let sys_cash_used = if sys_cash_ui > 0.0 { sys_cash_ui } else { sys_cash };
        let _ = sys_total_expected;

        // Cada linha só entra na conferência depois de preenchida: campo
        // vazio mostra "—" e não conta na diferença (nem no total).
        let rows = [
            (ui.global::<CashState>().get_cash_close_in_cash(), sys_cash_used),
            (ui.global::<CashState>().get_cash_close_in_pix(), sys_pix),
            (ui.global::<CashState>().get_cash_close_in_credit(), sys_credit),
            (ui.global::<CashState>().get_cash_close_in_debit(), sys_debit),
        ];
        let diffs: Vec<Option<f64>> = rows
            .iter()
            .map(|(raw, sys)| filled_diff(raw, *sys))
            .collect();

        let sys_total = sys_cash_used + sys_pix + sys_credit + sys_debit;
        // Total informado/diferença consideram só as linhas preenchidas.
        let in_total: f64 = rows
            .iter()
            .zip(&diffs)
            .filter(|(_, d)| d.is_some())
            .map(|((raw, _), _)| parse_amount(raw))
            .sum();
        let diff_total: Option<f64> = if diffs.iter().all(|d| d.is_none()) {
            None
        } else {
            Some(diffs.iter().flatten().sum())
        };

        ui.global::<CashState>().set_cash_close_sys_total(SharedString::from(fmt_brl(sys_total)));
        ui.global::<CashState>().set_cash_close_in_total(SharedString::from(fmt_brl(in_total)));
        ui.global::<CashState>().set_cash_close_diff_cash(diff_text(diffs[0]));
        ui.global::<CashState>().set_cash_close_diff_pix(diff_text(diffs[1]));
        ui.global::<CashState>().set_cash_close_diff_credit(diff_text(diffs[2]));
        ui.global::<CashState>().set_cash_close_diff_debit(diff_text(diffs[3]));
        ui.global::<CashState>().set_cash_close_diff_total(diff_text(diff_total));
        ui.global::<CashState>().set_cash_close_has_diff(diffs.iter().flatten().any(|d| d.abs() > 0.005));
    });
}

/// Diferença de uma linha: `None` enquanto o campo informado está vazio.
fn filled_diff(raw: &str, sys: f64) -> Option<f64> {
    if raw.trim().is_empty() {
        return None;
    }
    Some(parse_amount(raw) - sys)
}

/// Texto da coluna "diferença" (vazio = ainda não conferido).
fn diff_text(diff: Option<f64>) -> SharedString {
    match diff {
        Some(v) => SharedString::from(fmt_brl_signed(v)),
        None => SharedString::from(super::PENDING_DIFF),
    }
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
    ui.global::<CashState>().on_cash_close_confirm(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        let session_id_str = ui_ref.global::<CashState>().get_cash_summary().session_id.to_string();
        let Ok(session_id) = Uuid::parse_str(&session_id_str) else {
            show_toast(&ui_ref, "Nenhuma sessão aberta", "error");
            return;
        };
        let counted = parse_amount(&ui_ref.global::<CashState>().get_cash_close_in_cash());
        let notes = ui_ref.global::<CashState>().get_cash_close_notes().to_string();
        let has_diff = ui_ref.global::<CashState>().get_cash_close_has_diff();
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
                .close_session(cid, session_id, letaf_core::money::from_db_f64(counted), notes_opt)
                .await;
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                match result {
                    Ok(_) => {
                        ui.global::<CashState>().set_cash_show_close(false);
                        ui.global::<CashState>().set_cash_close_in_cash(SharedString::default());
                        ui.global::<CashState>().set_cash_close_in_pix(SharedString::default());
                        ui.global::<CashState>().set_cash_close_in_credit(SharedString::default());
                        ui.global::<CashState>().set_cash_close_in_debit(SharedString::default());
                        ui.global::<CashState>().set_cash_close_notes(SharedString::default());
                        show_toast(&ui, "Caixa Fechado", "success");
                        ui.global::<CashState>().invoke_cash_refresh();
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
    crate::format::parse_money_br_f64(s).unwrap_or(0.0)
}
