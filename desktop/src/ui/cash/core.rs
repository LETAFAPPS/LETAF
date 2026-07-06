use std::sync::Arc;
use rust_decimal::prelude::ToPrimitive;

use slint::ComponentHandle;

use letaf_core::cash::model::{
    SessionStatus, SessionSummary,
};

use crate::context::DesktopState;
use crate::MainWindow;

use super::ops::{setup_close_confirm, setup_close_recalc, setup_open_confirm, setup_sangria_confirm, setup_suprimento_confirm};
use super::view::apply_to_ui;

pub(crate) fn setup_cash(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<tokio::sync::Notify>,
) {
    setup_refresh(ui, state, handle);
    setup_open_confirm(ui, state, handle, sync_notify.clone());
    setup_sangria_confirm(ui, state, handle, sync_notify.clone());
    setup_suprimento_confirm(ui, state, handle, sync_notify.clone());
    setup_close_recalc(ui);
    setup_close_confirm(ui, state, handle, sync_notify);
}

// ── Helpers de formatação ────────────────────────────────────────
//
// Formatação monetária centralizada em `crate::format` (AI_RULES.md §8 —
// evitar duplicação). Apelidos locais mantidos pra preservar a leitura
// dos call-sites; a implementação é única.


pub(crate) fn parse_amount(s: &str) -> f64 {
    s.trim().replace(',', ".").parse::<f64>().unwrap_or(0.0).max(0.0)
}

pub(crate) fn fmt_duration(opened: chrono::NaiveDateTime, until: chrono::NaiveDateTime) -> String {
    let elapsed = until.signed_duration_since(opened);
    let total_min = elapsed.num_minutes().max(0);
    let h = total_min / 60;
    let m = total_min % 60;
    if h > 0 {
        format!("{}h {:02}min", h, m)
    } else {
        format!("{}min", m)
    }
}

pub(crate) fn now_local() -> chrono::NaiveDateTime {
    chrono::Local::now().naive_local()
}

/// Converte UTC naive → local naive (para exibição).
pub(crate) fn to_local(naive_utc: chrono::NaiveDateTime) -> chrono::NaiveDateTime {
    let utc = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(naive_utc, chrono::Utc);
    utc.with_timezone(&chrono::Local).naive_local()
}

// ── Refresh ──────────────────────────────────────────────────────

pub(crate) fn setup_refresh(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_cash_refresh(move || {
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            let active = state.cash_service.find_active(cid).await.ok().flatten();
            let recent = state.cash_service.find_recent(cid, 20).await.unwrap_or_default();
            let movements = if let Some(s) = active.as_ref() {
                state
                    .cash_service
                    .find_movements(cid, s.base.id)
                    .await
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            let summary = if let Some(s) = active.as_ref() {
                state
                    .cash_service
                    .session_summary(cid, s.base.id)
                    .await
                    .unwrap_or_default()
            } else {
                SessionSummary::default()
            };
            // Sugestão de troco: média dos `initial_change` das últimas
            // 5 sessões fechadas.
            let suggested = {
                let closed: Vec<f64> = recent
                    .iter()
                    .filter(|s| s.status == SessionStatus::Closed)
                    .take(5)
                    .map(|s| s.initial_change.to_f64().unwrap_or(0.0))
                    .collect();
                if closed.is_empty() {
                    100.0
                } else {
                    closed.iter().sum::<f64>() / (closed.len() as f64)
                }
            };
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                apply_to_ui(&ui, active.as_ref(), &recent, &movements, &summary, suggested);
            });
        });
    });
}

