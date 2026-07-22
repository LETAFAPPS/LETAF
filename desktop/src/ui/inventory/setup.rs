use std::sync::Arc;

use slint::{ComponentHandle, SharedString};
use uuid::Uuid;

use letaf_core::category::model::Category;
use letaf_core::product::model::Product;

use crate::context::DesktopState;
use crate::format::format_stock;
use crate::MainWindow;

use super::super::helpers::show_toast;
use super::view::apply_to_ui_from_cache;

pub(crate) type SharedCache = Arc<std::sync::Mutex<Vec<Product>>>;
pub(crate) type SharedCategories = Arc<std::sync::Mutex<Vec<Category>>>;

pub(crate) fn setup_inventory(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<tokio::sync::Notify>,
    sync_cycle_done: tokio::sync::watch::Receiver<u64>,
) {
    let cache: SharedCache = Arc::new(std::sync::Mutex::new(Vec::new()));
    let cats_cache: SharedCategories = Arc::new(std::sync::Mutex::new(Vec::new()));

    setup_refresh(ui, state, handle, cache.clone(), cats_cache.clone());
    setup_search(ui, cache.clone(), cats_cache.clone());
    setup_filter(ui, cache.clone(), cats_cache.clone());
    setup_request_add(ui, cache.clone());
    setup_request_edit(ui, cache.clone());
    setup_sync(ui, state, handle, sync_notify.clone());
    setup_confirm_adjust(ui, state, handle, sync_notify);
    setup_sync_listener(ui, state, handle, sync_cycle_done);
}

// ── Sincronização manual (botão "Sincronizar") ───────────────────
// Acorda o SyncWorker (envio imediato) e mantém o ícone girando até
// não haver mais pendências locais (ou timeout). Ao terminar, recarrega
// a lista para refletir o estado canônico do banco (§1/§7).
pub(crate) fn setup_sync(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<tokio::sync::Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_inventory_sync(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_inventory_syncing(true);
        }
        // Acorda o SyncWorker para um ciclo imediato.
        sync_notify.notify_one();

        let ui_weak = ui_weak.clone();
        let state = state.clone();
        handle.spawn(async move {
            use std::time::{Duration, Instant};
            let cid = state.company_id();
            let start = Instant::now();
            // Espera concluir: sem pendências locais E ≥600ms de feedback,
            // ou timeout de 8s (offline/servidor indisponível).
            loop {
                tokio::time::sleep(Duration::from_millis(300)).await;
                let pending = state
                    .product_service
                    .find_unsynced(cid)
                    .await
                    .map(|v| v.len())
                    .unwrap_or(0);
                let elapsed = start.elapsed();
                if elapsed >= Duration::from_secs(8)
                    || (pending == 0 && elapsed >= Duration::from_millis(600))
                {
                    break;
                }
            }
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    ui.set_inventory_syncing(false);
                    // Recarrega a lista (contadores/labels do estado atual).
                    ui.invoke_inventory_refresh();
                }
            });
        });
    });
}

// ── Listener do SyncWorker ───────────────────────────────────────
// Após cada ciclo de sync, recarrega a lista do banco — assim o
// header "AGUARDANDO SYNC" e o sync_label dos cards reagem em tempo
// real quando o SyncWorker termina de empurrar as alterações.
// AI_RULES §1/§7: UI consome o estado canônico do banco.
pub(crate) fn setup_sync_listener(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    mut cycle_done: tokio::sync::watch::Receiver<u64>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    handle.spawn(async move {
        loop {
            if cycle_done.changed().await.is_err() { break; }
            // Apenas atualiza o contador de pendentes (cheap) — não
            // precisa redecodificar imagens nem reordenar a lista.
            let cid = state.company_id();
            let pending = match state.product_service.find_unsynced(cid).await {
                Ok(v) => v.len() as i32,
                Err(e) => {
                    tracing::warn!("inventory sync listener: {e}");
                    continue;
                }
            };
            let ui_weak2 = ui_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak2.upgrade() else { return };
                // Atualiza só o campo `sync_pending_count` do header,
                // preservando o resto do `inventory_health` cacheado
                // (não força redecodificar miniaturas).
                let mut h = ui.get_inventory_health();
                h.sync_pending_count = pending;
                ui.set_inventory_health(h);
            });
        }
    });
}

// ── Search (filtra por nome, sem ir ao banco) ─────────────────────

pub(crate) fn setup_search(ui: &MainWindow, cache: SharedCache, cats_cache: SharedCategories) {
    let ui_weak = ui.as_weak();
    ui.on_inventory_search_changed(move |_q| {
        // O texto fica em `inventory-search` (Slint property), lido pelo
        // `apply_to_ui` direto. Re-renderiza com o cache atual.
        apply_to_ui_from_cache(&ui_weak, &cache, &cats_cache);
    });
}

// ── Filtro de status (abas Todos/Saudável/Baixo/Sem Estoque) ──────
// Mesmo padrão da busca: o valor fica em `inventory-filter` (Slint),
// lido pelo `apply_to_ui`. Re-renderiza a lista com o cache atual.
pub(crate) fn setup_filter(ui: &MainWindow, cache: SharedCache, cats_cache: SharedCategories) {
    let ui_weak = ui.as_weak();
    ui.on_inventory_filter_changed(move |_k| {
        apply_to_ui_from_cache(&ui_weak, &cache, &cats_cache);
    });
}

// ── Refresh ──────────────────────────────────────────────────────

pub(crate) fn setup_refresh(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    cache: SharedCache,
    cats_cache: SharedCategories,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_inventory_refresh(move || {
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let cache = cache.clone();
        let cats_cache = cats_cache.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            let products = state.product_service.find_all(cid).await.unwrap_or_default();
            let categories = state.category_service.find_all(cid).await.unwrap_or_default();
            // Badge da sidebar: produtos ativos esgotados (fora de estoque).
            let out = out_of_stock_count(&products);
            if let Ok(mut g) = cache.lock() { *g = products; }
            if let Ok(mut g) = cats_cache.lock() { *g = categories; }
            apply_to_ui_from_cache(&ui_weak, &cache, &cats_cache);
            let ui_weak = ui_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    ui.set_stock_out_count(out);
                }
            });
        });
    });
}

/// Conta os produtos ATIVOS fora de estoque (estoque não-ilimitado com
/// saldo ≤ 0). Fonte única do badge da sidebar (refresh + recompute).
pub(crate) fn out_of_stock_count(products: &[Product]) -> i32 {
    products
        .iter()
        .filter(|p| p.active && !p.unlimited_stock && p.stock_quantity <= 0.0)
        .count() as i32
}

// ── Ações: abrir modal "+ Estoque" / "Editar estoque" ────────────

pub(crate) fn setup_request_add(ui: &MainWindow, cache: SharedCache) {
    let ui_weak = ui.as_weak();
    ui.on_inventory_request_add(move |id_str| {
        let Some(ui) = ui_weak.upgrade() else { return };
        let Ok(id) = Uuid::parse_str(&id_str) else { return };
        let snapshot = cache.lock().ok().and_then(|g| g.iter().find(|p| p.base.id == id).cloned());
        let Some(p) = snapshot else { return };
        ui.set_stock_adjust_mode(SharedString::from("add"));
        ui.set_stock_adjust_product_id(SharedString::from(p.base.id.to_string()));
        ui.set_stock_adjust_product_name(SharedString::from(p.name.clone()));
        ui.set_stock_adjust_current(SharedString::from(format_stock(p.stock_quantity, &p.unit)));
        ui.set_stock_adjust_qty(SharedString::default());
        ui.set_stock_adjust_reason(SharedString::default());
        ui.set_stock_adjust_error(SharedString::default());
        ui.set_stock_adjust_show(true);
    });
}

pub(crate) fn setup_request_edit(ui: &MainWindow, cache: SharedCache) {
    let ui_weak = ui.as_weak();
    ui.on_inventory_request_edit(move |id_str| {
        let Some(ui) = ui_weak.upgrade() else { return };
        let Ok(id) = Uuid::parse_str(&id_str) else { return };
        let snapshot = cache.lock().ok().and_then(|g| g.iter().find(|p| p.base.id == id).cloned());
        let Some(p) = snapshot else { return };
        ui.set_stock_adjust_mode(SharedString::from("set"));
        ui.set_stock_adjust_product_id(SharedString::from(p.base.id.to_string()));
        ui.set_stock_adjust_product_name(SharedString::from(p.name.clone()));
        ui.set_stock_adjust_current(SharedString::from(format_stock(p.stock_quantity, &p.unit)));
        ui.set_stock_adjust_qty(SharedString::from(format!("{}", p.stock_quantity)));
        ui.set_stock_adjust_reason(SharedString::default());
        ui.set_stock_adjust_error(SharedString::default());
        ui.set_stock_adjust_show(true);
    });
}

// ── Confirmar ajuste de estoque ──────────────────────────────────

pub(crate) fn setup_confirm_adjust(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<tokio::sync::Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_inventory_confirm_adjust(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        let mode = ui_ref.get_stock_adjust_mode().to_string();
        let pid_str = ui_ref.get_stock_adjust_product_id().to_string();
        let qty_str = ui_ref.get_stock_adjust_qty().to_string();
        let reason = ui_ref.get_stock_adjust_reason().to_string();

        let Ok(pid) = Uuid::parse_str(&pid_str) else {
            ui_ref.set_stock_adjust_error(SharedString::from("Produto inválido"));
            return;
        };
        let parsed = qty_str.trim().replace(',', ".").parse::<f64>();
        let Ok(qty) = parsed else {
            ui_ref.set_stock_adjust_error(SharedString::from("Quantidade inválida"));
            return;
        };

        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let notify = sync_notify.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            // "set" exige consultar o estoque atual no service pra calcular delta
            // de forma robusta (não confiar no display da UI).
            let delta = if mode == "set" {
                let Ok(Some(p)) = state.product_service.find_by_id(cid, pid).await else {
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak.upgrade() {
                            ui.set_stock_adjust_error(SharedString::from("Produto não encontrado"));
                        }
                    });
                    return;
                };
                qty - p.stock_quantity
            } else {
                qty
            };

            if delta.abs() < 0.0005 {
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak.upgrade() {
                        ui.set_stock_adjust_error(SharedString::from(
                            "Nada a alterar (valor igual ao atual)",
                        ));
                    }
                });
                return;
            }

            let result = state.product_service.adjust_stock(cid, pid, delta).await;
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                match result {
                    Ok(()) => {
                        if !reason.trim().is_empty() {
                            tracing::info!(
                                "Stock adjusted (product {}, delta {}, reason: {})",
                                pid, delta, reason.trim()
                            );
                        }
                        ui.set_stock_adjust_show(false);
                        ui.set_stock_adjust_qty(SharedString::default());
                        ui.set_stock_adjust_reason(SharedString::default());
                        ui.set_stock_adjust_error(SharedString::default());
                        show_toast(&ui, "Estoque Atualizado", "success");
                        ui.invoke_inventory_refresh();
                        notify.notify_one();
                    }
                    Err(e) => {
                        ui.set_stock_adjust_error(SharedString::from(format!("{e}")));
                    }
                }
            });
        });
    });
}

