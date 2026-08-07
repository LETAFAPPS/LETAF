//! Callbacks da UI Slint para o domínio Insumos (matéria-prima).
//! Mesma arquitetura das demais features: a UI dispara callbacks; aqui
//! chamamos o `InsumoService` (validação + offline-first) e reaplicamos o
//! estado. Um cache local `Vec<Insumo>` alimenta a busca sem ir ao banco.

use std::sync::Arc;

use slint::{Color, ComponentHandle, ModelRc, SharedString, VecModel};
use tokio::sync::Notify;
use uuid::Uuid;

use letaf_core::insumo::model::Insumo;

use crate::context::DesktopState;
use crate::format::{format_stock, money_br, parse_money_br, parse_money_br_f64};
use crate::{InsumoData, InsumosState, MainWindow};

use super::helpers::{friendly_error, show_toast};

mod bulk;

type Cache = Arc<std::sync::Mutex<Vec<Insumo>>>;

pub(crate) fn setup_insumos(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let cache: Cache = Arc::new(std::sync::Mutex::new(Vec::new()));
    setup_refresh(ui, state, handle, cache.clone());
    setup_search(ui, cache.clone());
    setup_new(ui);
    setup_select(ui, cache.clone());
    setup_save(ui, state, handle, sync_notify.clone(), cache.clone(), false);
    setup_save(ui, state, handle, sync_notify.clone(), cache.clone(), true);
    setup_delete(ui, state, handle, sync_notify.clone(), cache);
    bulk::setup_bulk(ui, state, handle, sync_notify);
}

// ── Refresh + busca ──────────────────────────────────────────────

fn setup_refresh(ui: &MainWindow, state: &DesktopState, handle: &tokio::runtime::Handle, cache: Cache) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.global::<InsumosState>().on_refresh_insumos(move || {
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let cache = cache.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            let list = state.insumo_service.find_all(cid).await.unwrap_or_default();
            if let Ok(mut g) = cache.lock() { *g = list; }
            apply_to_ui(&ui_weak, &cache);
        });
    });
}

fn setup_search(ui: &MainWindow, cache: Cache) {
    let ui_weak = ui.as_weak();
    ui.global::<InsumosState>().on_search_insumos(move |_q| {
        apply_to_ui(&ui_weak, &cache);
    });
}

/// Reconstrói o modelo da lista a partir do cache, aplicando a busca atual.
fn apply_to_ui(ui_weak: &slint::Weak<MainWindow>, cache: &Cache) {
    let snapshot: Vec<Insumo> = cache.lock().ok().map(|g| g.clone()).unwrap_or_default();
    let ui_weak = ui_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        let needle = ui.global::<InsumosState>().get_insumos_search().to_string().trim().to_lowercase();
        let active_count = snapshot.iter().filter(|i| i.active).count() as i32;
        let rows: Vec<InsumoData> = snapshot
            .iter()
            .filter(|i| needle.is_empty() || i.name.to_lowercase().contains(&needle))
            .map(to_data)
            .collect();
        ui.global::<InsumosState>().set_insumos(ModelRc::new(VecModel::from(rows)));
        ui.global::<InsumosState>().set_insumos_active_count(active_count);
    });
}

/// Situação do estoque → (chave, cor) para o pontinho/quantidade do card.
fn status_of(i: &Insumo) -> (&'static str, Color) {
    if i.stock_quantity <= 0.0 {
        ("out", Color::from_rgb_u8(0xC6, 0x28, 0x28))
    } else if i.min_stock > 0.0 && i.stock_quantity < i.min_stock {
        ("low", Color::from_rgb_u8(0xF5, 0x7F, 0x17))
    } else {
        ("ok", Color::from_rgb_u8(0x2E, 0x7D, 0x32))
    }
}

fn to_data(i: &Insumo) -> InsumoData {
    let (status, color) = status_of(i);
    let cost_display = i.cost_price.map(money_br).unwrap_or_else(|| "—".to_string());
    InsumoData {
        id: SharedString::from(i.base.id.to_string()),
        name: SharedString::from(i.name.clone()),
        description: SharedString::from(i.description.clone().unwrap_or_default()),
        unit: SharedString::from(i.unit.clone()),
        stock: SharedString::from(fmt_num(i.stock_quantity)),
        stock_display: SharedString::from(format_stock(i.stock_quantity, &i.unit)),
        min_stock: SharedString::from(fmt_num(i.min_stock)),
        min_display: SharedString::from(format!("mín. {}", format_stock(i.min_stock, &i.unit))),
        cost: SharedString::from(i.cost_price.map(|d| d.to_string()).unwrap_or_default()),
        cost_display: SharedString::from(cost_display),
        barcode: SharedString::from(i.barcode.clone().unwrap_or_default()),
        active: i.active,
        status: SharedString::from(status),
        status_color: color,
    }
}

fn fmt_num(v: f64) -> String {
    format!("{v}").replace('.', ",")
}

// ── Novo / selecionar ────────────────────────────────────────────

fn setup_new(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.global::<InsumosState>().on_request_new_insumo(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        let g = ui.global::<InsumosState>();
        g.set_editing_insumo_id(SharedString::default());
        g.set_selected_insumo_id(SharedString::default());
        g.set_insumo_name(SharedString::default());
        g.set_insumo_unit(SharedString::from("un"));
        g.set_insumo_description(SharedString::default());
        g.set_insumo_cost(SharedString::default());
        g.set_insumo_barcode(SharedString::default());
        g.set_insumo_stock(SharedString::default());
        g.set_insumo_min_stock(SharedString::default());
        g.set_insumo_active(true);
        g.set_insumo_save_error(SharedString::default());
    });
}

fn setup_select(ui: &MainWindow, cache: Cache) {
    let ui_weak = ui.as_weak();
    ui.global::<InsumosState>().on_select_insumo(move |id_str| {
        let Some(ui) = ui_weak.upgrade() else { return };
        let Ok(id) = Uuid::parse_str(&id_str) else { return };
        let found = cache.lock().ok().and_then(|g| g.iter().find(|i| i.base.id == id).cloned());
        let Some(i) = found else { return };
        let g = ui.global::<InsumosState>();
        g.set_editing_insumo_id(SharedString::from(i.base.id.to_string()));
        g.set_selected_insumo_id(SharedString::from(i.base.id.to_string()));
        g.set_insumo_name(SharedString::from(i.name.clone()));
        g.set_insumo_unit(SharedString::from(i.unit.clone()));
        g.set_insumo_description(SharedString::from(i.description.clone().unwrap_or_default()));
        g.set_insumo_cost(SharedString::from(i.cost_price.map(|d| d.to_string()).unwrap_or_default()));
        g.set_insumo_barcode(SharedString::from(i.barcode.clone().unwrap_or_default()));
        g.set_insumo_stock(SharedString::from(fmt_num(i.stock_quantity)));
        g.set_insumo_min_stock(SharedString::from(fmt_num(i.min_stock)));
        g.set_insumo_active(i.active);
        g.set_insumo_save_error(SharedString::default());
    });
}

// ── Salvar (criar/atualizar) ─────────────────────────────────────

/// Lê o formulário. `is_update = true` liga o callback de atualização.
fn setup_save(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
    cache: Cache,
    is_update: bool,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    let body = move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        let g = ui.global::<InsumosState>();
        let name = g.get_insumo_name().to_string();
        let unit = g.get_insumo_unit().to_string();
        let description = opt(g.get_insumo_description().to_string());
        let barcode = opt(g.get_insumo_barcode().to_string());
        let active = g.get_insumo_active();
        let stock = parse_money_br_f64(&g.get_insumo_stock()).unwrap_or(0.0);
        let min_stock = parse_money_br_f64(&g.get_insumo_min_stock()).unwrap_or(0.0);
        let cost = {
            let raw = g.get_insumo_cost().to_string();
            if raw.trim().is_empty() { None } else { parse_money_br(&raw) }
        };
        let id_str = g.get_editing_insumo_id().to_string();

        let ui_weak = ui.as_weak();
        let state = state.clone();
        let notify = sync_notify.clone();
        let cache = cache.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            let result = if is_update {
                match Uuid::parse_str(&id_str) {
                    Ok(id) => state.insumo_service
                        .update(cid, id, name, description, unit, stock, min_stock, cost, barcode, active)
                        .await,
                    Err(_) => return,
                }
            } else {
                state.insumo_service
                    .create(cid, name, description, unit, stock, min_stock, cost, barcode, active)
                    .await
            };
            match result {
                Ok(i) => {
                    // Atualiza o cache e a lista; mantém o insumo salvo selecionado.
                    let list = state.insumo_service.find_all(cid).await.unwrap_or_default();
                    if let Ok(mut cg) = cache.lock() { *cg = list; }
                    notify.notify_one();
                    let new_id = i.base.id.to_string();
                    let name_saved = i.name.clone();
                    let ui_weak2 = ui_weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak2.upgrade() else { return };
                        let g = ui.global::<InsumosState>();
                        g.set_editing_insumo_id(SharedString::from(new_id.clone()));
                        g.set_selected_insumo_id(SharedString::from(new_id));
                        g.set_insumo_save_error(SharedString::default());
                        show_toast(&ui, &format!("Insumo '{name_saved}' salvo"), "success");
                    });
                    apply_to_ui(&ui_weak, &cache);
                }
                Err(e) => {
                    let msg = friendly_error(&e);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak.upgrade() {
                            ui.global::<InsumosState>().set_insumo_save_error(SharedString::from(msg));
                        }
                    });
                }
            }
        });
    };

    if is_update {
        ui.global::<InsumosState>().on_update_insumo(body);
    } else {
        ui.global::<InsumosState>().on_add_insumo(body);
    }
}

fn setup_delete(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
    cache: Cache,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.global::<InsumosState>().on_delete_insumo(move |id_str| {
        let Ok(id) = Uuid::parse_str(&id_str) else { return };
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let notify = sync_notify.clone();
        let cache = cache.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            match state.insumo_service.soft_delete(cid, id).await {
                Ok(()) => {
                    let list = state.insumo_service.find_all(cid).await.unwrap_or_default();
                    if let Ok(mut cg) = cache.lock() { *cg = list; }
                    notify.notify_one();
                    let ui_weak2 = ui_weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak2.upgrade() {
                            let g = ui.global::<InsumosState>();
                            g.set_editing_insumo_id(SharedString::default());
                            g.set_selected_insumo_id(SharedString::default());
                            show_toast(&ui, "Insumo excluído", "success");
                        }
                    });
                    apply_to_ui(&ui_weak, &cache);
                }
                Err(e) => {
                    let msg = friendly_error(&e);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak.upgrade() {
                            show_toast(&ui, &msg, "error");
                        }
                    });
                }
            }
        });
    });
}

fn opt(s: String) -> Option<String> {
    let t = s.trim();
    if t.is_empty() { None } else { Some(t.to_string()) }
}
