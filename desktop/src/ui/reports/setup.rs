use std::sync::Arc;

use slint::ComponentHandle;


use crate::context::DesktopState;
use crate::MainWindow;

use super::super::helpers::show_toast;
use super::state::{Caches, ReportState, Shared};
use chrono::Datelike;

use super::snapshot::{apply_to_ui, build_snapshot};
use crate::ReportsState;

pub(crate) fn setup_reports(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_cycle_done: tokio::sync::watch::Receiver<u64>,
) {
    let rs: Shared<ReportState> = Arc::new(std::sync::Mutex::new(ReportState::default()));
    let caches = Caches {
        orders: Arc::new(std::sync::Mutex::new(Vec::new())),
        fiado_aberto: Arc::new(std::sync::Mutex::new(Vec::new())),
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
    ui.global::<ReportsState>().on_report_refresh(move || {
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let rs = rs.clone();
        let caches = caches.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            // Só o intervalo que a tela pode PEDIR, não o histórico inteiro.
            // A janela mais ampla é "ano" comparado com o ano anterior, então
            // 1º de janeiro do ano passado cobre qualquer período selecionável
            // — e a troca de período continua instantânea, direto do cache.
            // Antes era `find_all`: todo o histórico era transportado e
            // recortado em Rust, com custo crescendo para sempre.
            let hoje = letaf_core::tz::today();
            let inicio = chrono::NaiveDate::from_ymd_opt(hoje.year() - 1, 1, 1).unwrap_or(hoje);
            let orders = state
                .order_service
                .find_in_period(cid, inicio, hoje)
                .await
                .unwrap_or_default();
            // O fiado em aberto não tem recorte de período: um pedido de três
            // anos atrás ainda pode estar em aberto. Conjunto pequeno.
            let fiado_aberto = state
                .order_service
                .find_unpaid_wallet(cid)
                .await
                .unwrap_or_default();
            let products = state.product_service.find_all(cid).await.unwrap_or_default();
            let categories = state.category_service.find_all(cid).await.unwrap_or_default();
            let customers = state.customer_service.find_all(cid).await.unwrap_or_default();
            if let Ok(mut g) = caches.orders.lock() { *g = orders; }
            if let Ok(mut g) = caches.fiado_aberto.lock() { *g = fiado_aberto; }
            if let Ok(mut g) = caches.products.lock() { *g = products; }
            if let Ok(mut g) = caches.categories.lock() { *g = categories; }
            if let Ok(mut g) = caches.customers.lock() { *g = customers; }
            reapply(&ui_weak, &rs, &caches);
        });
    });
}

pub(crate) fn setup_set_type(ui: &MainWindow, rs: Shared<ReportState>, caches: Caches) {
    let ui_weak = ui.as_weak();
    ui.global::<ReportsState>().on_report_set_type(move |key| {
        if let Ok(mut g) = rs.lock() { g.kind = key.to_string(); }
        reapply(&ui_weak, &rs, &caches);
    });
}

pub(crate) fn setup_set_period(ui: &MainWindow, rs: Shared<ReportState>, caches: Caches) {
    let ui_weak = ui.as_weak();
    ui.global::<ReportsState>().on_report_set_period(move |key| {
        if let Ok(mut g) = rs.lock() { g.period = key.to_string(); }
        reapply(&ui_weak, &rs, &caches);
    });
}

pub(crate) fn setup_export(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.global::<ReportsState>().on_report_export(move || {
        if let Some(ui) = ui_weak.upgrade() {
            show_toast(&ui, "Exportação em desenvolvimento", "info");
        }
    });
}

pub(crate) fn setup_sync_listener(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    mut cycle_done: tokio::sync::watch::Receiver<u64>,
    rs: Shared<ReportState>,
    caches: Caches,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    handle.spawn(async move {
        loop {
            if cycle_done.changed().await.is_err() { break; }
            let visible = {
                let ui_weak2 = ui_weak.clone();
                let (tx, rx) = tokio::sync::oneshot::channel();
                let _ = slint::invoke_from_event_loop(move || {
                    let active = ui_weak2
                        .upgrade()
                        .map(|u| u.get_active_tab().to_string())
                        .unwrap_or_default();
                    let _ = tx.send(active == "reports");
                });
                rx.await.unwrap_or(false)
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
    // Trabalha sob os locks em vez de CLONAR os caches: `reapply` roda a cada
    // troca de período e de tipo de relatório, e clonar os pedidos do ano
    // inteiro (com os itens) a cada clique era um custo silencioso. Nada aqui
    // dá `.await`, então segurar os locks não trava o executor.
    let vazio: Vec<letaf_core::order::model::Order> = Vec::new();
    let orders = caches.orders.lock();
    let fiado = caches.fiado_aberto.lock();
    let products = caches.products.lock();
    let categories = caches.categories.lock();
    let customers = caches.customers.lock();
    let snap = build_snapshot(
        &state,
        orders.as_deref().unwrap_or(&vazio),
        fiado.as_deref().unwrap_or(&vazio),
        products.as_deref().map(|v| &v[..]).unwrap_or(&[]),
        categories.as_deref().map(|v| &v[..]).unwrap_or(&[]),
        customers.as_deref().map(|v| &v[..]).unwrap_or(&[]),
    );
    let ui_weak = ui_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_weak.upgrade() {
            apply_to_ui(&ui, &snap);
        }
    });
}

