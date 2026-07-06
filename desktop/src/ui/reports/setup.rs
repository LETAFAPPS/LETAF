use std::sync::Arc;

use slint::ComponentHandle;


use crate::context::DesktopState;
use crate::MainWindow;

use super::super::helpers::show_toast;
use super::state::{Caches, ReportState, Shared};
use super::snapshot::{apply_to_ui, build_snapshot};

pub(crate) fn setup_reports(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_cycle_done: Arc<tokio::sync::Notify>,
) {
    let rs: Shared<ReportState> = Arc::new(std::sync::Mutex::new(ReportState::default()));
    let caches = Caches {
        orders: Arc::new(std::sync::Mutex::new(Vec::new())),
        products: Arc::new(std::sync::Mutex::new(Vec::new())),
        categories: Arc::new(std::sync::Mutex::new(Vec::new())),
        customers: Arc::new(std::sync::Mutex::new(Vec::new())),
    };
    setup_refresh(ui, state, handle, rs.clone(), caches.clone());
    setup_set_type(ui, rs.clone(), caches.clone());
    setup_set_period(ui, rs.clone(), caches.clone());
    setup_export(ui);
    setup_sync_listener(ui, state, handle, sync_cycle_done, rs, caches);
}

// ── Refresh ─────────────────────────────────────────────────────

pub(crate) fn setup_refresh(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    rs: Shared<ReportState>,
    caches: Caches,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_report_refresh(move || {
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let rs = rs.clone();
        let caches = caches.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            let orders = state.order_service.find_all(cid).await.unwrap_or_default();
            let products = state.product_service.find_all(cid).await.unwrap_or_default();
            let categories = state.category_service.find_all(cid).await.unwrap_or_default();
            let customers = state.customer_service.find_all(cid).await.unwrap_or_default();
            if let Ok(mut g) = caches.orders.lock() { *g = orders; }
            if let Ok(mut g) = caches.products.lock() { *g = products; }
            if let Ok(mut g) = caches.categories.lock() { *g = categories; }
            if let Ok(mut g) = caches.customers.lock() { *g = customers; }
            reapply(&ui_weak, &rs, &caches);
        });
    });
}

pub(crate) fn setup_set_type(ui: &MainWindow, rs: Shared<ReportState>, caches: Caches) {
    let ui_weak = ui.as_weak();
    ui.on_report_set_type(move |key| {
        if let Ok(mut g) = rs.lock() { g.kind = key.to_string(); }
        reapply(&ui_weak, &rs, &caches);
    });
}

pub(crate) fn setup_set_period(ui: &MainWindow, rs: Shared<ReportState>, caches: Caches) {
    let ui_weak = ui.as_weak();
    ui.on_report_set_period(move |key| {
        if let Ok(mut g) = rs.lock() { g.period = key.to_string(); }
        reapply(&ui_weak, &rs, &caches);
    });
}

pub(crate) fn setup_export(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_report_export(move || {
        if let Some(ui) = ui_weak.upgrade() {
            show_toast(&ui, "Exportação em desenvolvimento", "info");
        }
    });
}

pub(crate) fn setup_sync_listener(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    cycle_done: Arc<tokio::sync::Notify>,
    rs: Shared<ReportState>,
    caches: Caches,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    handle.spawn(async move {
        loop {
            cycle_done.notified().await;
            let visible = {
                let ui_weak2 = ui_weak.clone();
                let (tx, rx) = std::sync::mpsc::channel();
                let _ = slint::invoke_from_event_loop(move || {
                    let active = ui_weak2
                        .upgrade()
                        .map(|u| u.get_active_tab().to_string())
                        .unwrap_or_default();
                    let _ = tx.send(active == "reports");
                });
                rx.recv().unwrap_or(false)
            };
            if !visible { continue; }
            let cid = state.company_id();
            let orders = state.order_service.find_all(cid).await.unwrap_or_default();
            let products = state.product_service.find_all(cid).await.unwrap_or_default();
            let categories = state.category_service.find_all(cid).await.unwrap_or_default();
            let customers = state.customer_service.find_all(cid).await.unwrap_or_default();
            if let Ok(mut g) = caches.orders.lock() { *g = orders; }
            if let Ok(mut g) = caches.products.lock() { *g = products; }
            if let Ok(mut g) = caches.categories.lock() { *g = categories; }
            if let Ok(mut g) = caches.customers.lock() { *g = customers; }
            reapply(&ui_weak, &rs, &caches);
        }
    });
}

// ── Reapply ─────────────────────────────────────────────────────

pub(crate) fn reapply(ui_weak: &slint::Weak<MainWindow>, rs: &Shared<ReportState>, caches: &Caches) {
    let state = rs.lock().ok().map(|g| g.clone()).unwrap_or_default();
    let orders = caches.orders.lock().ok().map(|g| g.clone()).unwrap_or_default();
    let products = caches.products.lock().ok().map(|g| g.clone()).unwrap_or_default();
    let categories = caches.categories.lock().ok().map(|g| g.clone()).unwrap_or_default();
    let customers = caches.customers.lock().ok().map(|g| g.clone()).unwrap_or_default();
    let snap = build_snapshot(&state, &orders, &products, &categories, &customers);
    let ui_weak = ui_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_weak.upgrade() {
            apply_to_ui(&ui, &snap);
        }
    });
}

