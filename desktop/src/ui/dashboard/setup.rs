use std::sync::Arc;

use slint::ComponentHandle;


use crate::context::DesktopState;
use crate::MainWindow;
use super::snapshot::{apply_to_ui, build_snapshot};

pub(crate) fn setup_dashboard(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_cycle_done: Arc<tokio::sync::Notify>,
) {
    setup_refresh(ui, state, handle);
    setup_sync_listener(ui, state, handle, sync_cycle_done);
}

// ── Refresh ──────────────────────────────────────────────────────

pub(crate) fn setup_refresh(ui: &MainWindow, state: &DesktopState, handle: &tokio::runtime::Handle) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_dashboard_refresh(move || {
        // Período selecionado no filtro — lido na thread da UI.
        let period = ui_weak
            .upgrade()
            .map(|u| u.get_dashboard_period().to_string())
            .unwrap_or_else(|| "week".to_string());
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            let orders = state.order_service.find_all(cid).await.unwrap_or_default();
            // Estado do SyncWorker (online/fase/pendentes) — fonte do
            // card "Sincronização" (Sincronizado/Sincronizando/Aguardando).
            let st = state.sync_status.snapshot();
            let pending = st.pending_count as i32;
            let online = st.online;
            let syncing = st.phase == crate::sync::status::SyncPhase::Syncing;
            let snapshot = build_snapshot(&orders, pending, online, syncing, &period);
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    apply_to_ui(&ui, &snapshot);
                }
            });
        });
    });
}

// ── Listener do SyncWorker (apenas re-roda o refresh) ────────────

pub(crate) fn setup_sync_listener(
    ui: &MainWindow,
    _state: &DesktopState,
    handle: &tokio::runtime::Handle,
    cycle_done: Arc<tokio::sync::Notify>,
) {
    let ui_weak = ui.as_weak();
    handle.spawn(async move {
        loop {
            cycle_done.notified().await;
            let ui_weak2 = ui_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak2.upgrade() {
                    // Só recomputa quando a tela está visível para evitar
                    // trabalho inútil.
                    if ui.get_active_tab() == "dashboard" {
                        ui.invoke_dashboard_refresh();
                    }
                }
            });
        }
    });
}

