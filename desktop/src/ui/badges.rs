//! Recompute em tempo real dos badges da sidebar (Pedidos, Financeiro,
//! Estoque e Assinatura).
//!
//! Um ÚNICO ouvinte reage ao `badges_dirty` que o SyncWorker dispara ao
//! fim de cada ciclo. Como toda escrita local aciona um ciclo de sync
//! (offline-first §7.3), os badges refletem qualquer mudança — local ou
//! vinda do pull — sem o operador trocar de aba.
//!
//! Usa um Notify DEDICADO (não o `cycle_done` compartilhado por 7 telas):
//! com um só ouvinte, `notify_one` bufferiza o permit e nunca perde um
//! ciclo — garantindo o "tempo real" de fato.
use std::sync::Arc;

use slint::ComponentHandle;
use tokio::sync::Notify;

use crate::context::DesktopState;
use crate::MainWindow;

/// Lê o SQLite local, recalcula os 4 contadores e pinta na UI num único
/// `invoke_from_event_loop`. Toda derivação no Rust (§3/§11); a UI só
/// exibe o número. Isolado por `company_id`.
pub(crate) async fn refresh_all_badges(ui_weak: &slint::Weak<MainWindow>, state: &DesktopState) {
    let cid = state.company_id();
    let today = chrono::Local::now().date_naive();

    let orders = state.order_service.find_all(cid).await.unwrap_or_default();
    let entries = state.finance_service.find_all(cid).await.unwrap_or_default();
    let products = state.product_service.find_all(cid).await.unwrap_or_default();
    let sub_pending = state
        .subscription_service
        .pending_summary(cid, today)
        .await
        .map(|s| s.action_count as i32)
        .unwrap_or(0);

    let orders_n = super::orders::active_orders_count(&orders);
    let overdue_n = super::finance::overdue_count(&entries);
    let stock_n = super::inventory::out_of_stock_count(&products);

    let ui_weak = ui_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_orders_active_count(orders_n);
            ui.set_finance_overdue_count(overdue_n);
            ui.set_stock_out_count(stock_n);
            ui.set_subscription_pending_count(sub_pending);
        }
    });
}

/// Ouve o `badges_dirty` (um ciclo de sync terminou) e recalcula. Pinta
/// uma vez no startup para os badges já aparecerem sem abrir as abas.
pub(crate) fn setup_badges_listener(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    badges_dirty: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    handle.spawn(async move {
        refresh_all_badges(&ui_weak, &state).await;
        loop {
            badges_dirty.notified().await;
            refresh_all_badges(&ui_weak, &state).await;
        }
    });
}
