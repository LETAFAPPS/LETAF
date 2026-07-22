
use slint::ComponentHandle;


use crate::context::DesktopState;
use crate::MainWindow;
use super::snapshot::{apply_to_ui, build_snapshot};

pub(crate) fn setup_dashboard(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_cycle_done: tokio::sync::watch::Receiver<u64>,
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
            // Fuso da LOJA (mesmo campo que o servidor usa em
            // `availability::local_now`): `created_at` é UTC, e sem converter
            // uma venda das 21h em BRT cairia no dia seguinte. `_light` não
            // carrega os blobs de logo/capa (§13).
            let utc_offset = state
                .company_service
                .find_by_id_light(cid)
                .await
                .ok()
                .flatten()
                .map(|c| c.utc_offset_minutes)
                .unwrap_or(0);
            // Estado do SyncWorker (online/fase/pendentes) — fonte do
            // card "Sincronização" (Sincronizado/Sincronizando/Aguardando).
            let st = state.sync_status.snapshot();
            let pending = st.pending_count as i32;
            let online = st.online;
            let syncing = st.phase == crate::sync::status::SyncPhase::Syncing;
            let snapshot =
                build_snapshot(&orders, pending, online, syncing, &period, utc_offset);
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
    mut cycle_done: tokio::sync::watch::Receiver<u64>,
) {
    let ui_weak = ui.as_weak();
    handle.spawn(async move {
        loop {
            if cycle_done.changed().await.is_err() { break; }
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

