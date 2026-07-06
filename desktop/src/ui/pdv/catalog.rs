use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use slint::{ComponentHandle, SharedPixelBuffer, SharedString};
use uuid::Uuid;


use crate::context::DesktopState;
use crate::MainWindow;

use super::state::{CartItem, parse_amount, PdvState};
use super::cart::{setup_clear_cart, setup_dec_line, setup_inc_line, setup_remove_line, slint_row_count, slint_row_data};
use super::finalize::setup_finalize;
use super::customer::{setup_clear_customer, setup_customer_picker, setup_customer_search, setup_pick_customer, setup_use_address};
use super::view::apply_state_to_ui;

pub(crate) fn setup_pdv(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<tokio::sync::Notify>,
) {
    let pdv = Arc::new(Mutex::new(PdvState::new()));
    setup_refresh(ui, state, handle, pdv.clone());
    setup_search(ui, state, handle, pdv.clone());
    setup_submit_search(ui, pdv.clone());
    setup_toggle_category(ui, pdv.clone());
    setup_clear_categories(ui, pdv.clone());
    setup_cats_width_changed(ui, pdv.clone());
    setup_add_product(ui, state, handle, pdv.clone());
    setup_inc_line(ui, pdv.clone());
    setup_dec_line(ui, pdv.clone());
    setup_remove_line(ui, pdv.clone());
    setup_clear_cart(ui, pdv.clone());
    setup_finalize(ui, state, handle, pdv.clone(), sync_notify);
    setup_customer_picker(ui, pdv.clone());
    setup_customer_search(ui, pdv.clone());
    setup_pick_customer(ui, state, handle, pdv.clone());
    setup_clear_customer(ui, pdv.clone());
    setup_pdv_config_confirm(ui, pdv.clone());
    setup_use_address(ui);
    setup_discount_changed(ui, pdv.clone());
    setup_additional_changed(ui, pdv.clone());
    setup_amount_paid_changed(ui, pdv.clone());
    setup_recalc(ui, pdv);
}

pub(crate) fn setup_recalc(ui: &MainWindow, pdv: Arc<Mutex<PdvState>>) {
    let ui_weak = ui.as_weak();
    ui.on_pdv_recalc(move || {
        if let Some(ui) = ui_weak.upgrade() {
            apply_state_to_ui(&ui, &pdv);
        }
    });
}

pub(crate) fn setup_discount_changed(ui: &MainWindow, pdv: Arc<Mutex<PdvState>>) {
    let ui_weak = ui.as_weak();
    ui.on_pdv_discount_changed(move |raw| {
        if let Ok(mut g) = pdv.lock() {
            g.discount_value = parse_amount(raw.as_str());
        }
        if let Some(ui) = ui_weak.upgrade() {
            apply_state_to_ui(&ui, &pdv);
        }
    });
}

pub(crate) fn setup_additional_changed(ui: &MainWindow, pdv: Arc<Mutex<PdvState>>) {
    let ui_weak = ui.as_weak();
    ui.on_pdv_additional_changed(move |raw| {
        if let Ok(mut g) = pdv.lock() {
            g.additional_value = parse_amount(raw.as_str());
        }
        if let Some(ui) = ui_weak.upgrade() {
            apply_state_to_ui(&ui, &pdv);
        }
    });
}

pub(crate) fn setup_amount_paid_changed(ui: &MainWindow, pdv: Arc<Mutex<PdvState>>) {
    let ui_weak = ui.as_weak();
    ui.on_pdv_amount_paid_changed(move |raw| {
        if let Ok(mut g) = pdv.lock() {
            g.amount_paid = parse_amount(raw.as_str());
        }
        if let Some(ui) = ui_weak.upgrade() {
            apply_state_to_ui(&ui, &pdv);
        }
    });
}

pub(crate) fn setup_refresh(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    pdv: Arc<Mutex<PdvState>>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_pdv_refresh(move || {
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let pdv = pdv.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            let products = state.product_service.find_all(cid).await.unwrap_or_default();
            let categories = state.category_service.find_all(cid).await.unwrap_or_default();
            let customers = state.customer_service.find_all(cid).await.unwrap_or_default();
            // Decodifica imagens AQUI (fora do event loop Slint) —
            // `SharedPixelBuffer` é `Send` então passamos para a
            // closure do `invoke_from_event_loop`. Decodificação só
            // acontece uma vez por refresh — itens no cache não
            // são reprocessados.
            let mut new_cache: HashMap<Uuid, SharedPixelBuffer<slint::Rgba8Pixel>> = HashMap::new();
            for p in &products {
                if let Some(b64) = p.image_data.as_deref().filter(|s| !s.is_empty()) {
                    if let Some(buf) = super::super::image::decode_pixel_buffer(b64) {
                        new_cache.insert(p.base.id, buf);
                    }
                }
            }
            let cat_tuples: Vec<(Uuid, String)> = categories
                .into_iter()
                .map(|c| (c.base.id, c.name))
                .collect();
            let customer_tuples: Vec<(Uuid, String, Option<String>, Option<String>)> = customers
                .into_iter()
                .map(|c| (c.base.id, c.name, c.phone, c.document))
                .collect();
            {
                let mut g = pdv.lock().expect("pdv state poisoned");
                g.products_all = products;
                g.categories = cat_tuples;
                g.customers_all = customer_tuples;
                g.image_cache = new_cache;
            }
            let pdv = pdv.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    apply_state_to_ui(&ui, &pdv);
                }
            });
        });
    });
}

/// `search-changed` faz duas coisas:
/// 1. Atualiza o filtro textual da grid (busca por nome/barcode parcial).
/// 2. **Auto-add do scanner**: se o texto bate EXATAMENTE com o
///    barcode de algum produto, adiciona ao carrinho e limpa o input.
///    Funciona porque leitores de barcode "datilografam" o código
///    rapidamente caracter por caracter — quando o último char
///    completa o barcode, o lookup acerta e zeramos.
pub(crate) fn setup_search(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    pdv: Arc<Mutex<PdvState>>,
) {
    let _state = state.clone();
    let _handle = handle.clone();
    let ui_weak = ui.as_weak();
    ui.on_pdv_search_changed(move |q| {
        let trimmed = q.trim().to_string();
        // Auto-add por barcode (match exato).
        let matched_pid = {
            let g = match pdv.lock() { Ok(g) => g, Err(_) => return };
            if trimmed.is_empty() { None } else {
                g.products_all.iter()
                    .find(|p| p.barcode.as_deref().map(|b| b == trimmed).unwrap_or(false))
                    .map(|p| p.base.id)
            }
        };
        if let Some(pid) = matched_pid {
            add_to_cart_simple(&pdv, pid);
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_pdv_search_text(SharedString::default());
                if let Ok(mut g) = pdv.lock() { g.search_query.clear(); }
                apply_state_to_ui(&ui, &pdv);
            }
            return;
        }
        // Sem match → atualiza filtro textual.
        if let Ok(mut g) = pdv.lock() { g.search_query = q.to_string(); }
        if let Some(ui) = ui_weak.upgrade() {
            apply_state_to_ui(&ui, &pdv);
        }
    });
}

pub(crate) fn setup_submit_search(ui: &MainWindow, pdv: Arc<Mutex<PdvState>>) {
    let ui_weak = ui.as_weak();
    ui.on_pdv_submit_search(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        let q = ui_ref.get_pdv_search_text().to_string();
        let trimmed = q.trim().to_string();
        if trimmed.is_empty() { return; }
        let matched = pdv.lock().ok().and_then(|g| {
            g.products_all.iter()
                .find(|p| p.barcode.as_deref().map(|b| b == trimmed).unwrap_or(false))
                .map(|p| p.base.id)
        });
        if let Some(pid) = matched {
            add_to_cart_simple(&pdv, pid);
            ui_ref.set_pdv_search_text(SharedString::default());
            if let Ok(mut g) = pdv.lock() { g.search_query.clear(); }
            apply_state_to_ui(&ui_ref, &pdv);
        }
    });
}

pub(crate) fn setup_toggle_category(ui: &MainWindow, pdv: Arc<Mutex<PdvState>>) {
    let ui_weak = ui.as_weak();
    ui.on_pdv_toggle_category(move |id| {
        let Ok(uuid) = Uuid::parse_str(id.as_str()) else { return };
        if let Ok(mut g) = pdv.lock() {
            if let Some(pos) = g.active_category_ids.iter().position(|x| *x == uuid) {
                g.active_category_ids.remove(pos);
            } else {
                g.active_category_ids.push(uuid);
            }
        }
        if let Some(ui) = ui_weak.upgrade() {
            apply_state_to_ui(&ui, &pdv);
        }
    });
}

pub(crate) fn setup_clear_categories(ui: &MainWindow, pdv: Arc<Mutex<PdvState>>) {
    let ui_weak = ui.as_weak();
    ui.on_pdv_clear_categories(move || {
        if let Ok(mut g) = pdv.lock() { g.active_category_ids.clear(); }
        if let Some(ui) = ui_weak.upgrade() {
            apply_state_to_ui(&ui, &pdv);
        }
    });
}

/// `pdv-cats-width-changed` — Slint avisa quando o painel de
/// categorias muda de largura. Guarda em `cats_width` e re-renderiza:
/// `apply_state_to_ui` decide se quebra os chips em duas linhas. Para
/// evitar churn em pixels triviais, só renderiza quando muda >= 4 px.
pub(crate) fn setup_cats_width_changed(ui: &MainWindow, pdv: Arc<Mutex<PdvState>>) {
    let ui_weak = ui.as_weak();
    ui.on_pdv_cats_width_changed(move |w| {
        let prev = pdv.lock().ok().map(|g| g.cats_width).unwrap_or(0.0);
        if (prev - w).abs() < 4.0 { return; }
        if let Ok(mut g) = pdv.lock() { g.cats_width = w; }
        // Re-renderiza DIFERIDO: `apply_state_to_ui` reescreve modelos
        // (categorias/produtos); fazê-lo SÍNCRONO dentro do `changed width`
        // reentra no layout do Slint ("Recursion detected") quando o PDV
        // monta numa fase instável (ex.: PDV como tela inicial no startup).
        let ui_weak = ui_weak.clone();
        let pdv = pdv.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_weak.upgrade() {
                apply_state_to_ui(&ui, &pdv);
            }
        });
    });
}

/// `pdv-add-product` — operador clicou num card. Se o produto tem
/// variações/adicionais, dispara o `ProductConfiguratorModal` no
/// contexto PDV. Senão, adiciona direto ao carrinho.
pub(crate) fn setup_add_product(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    pdv: Arc<Mutex<PdvState>>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_pdv_add_product(move |id| {
        let Ok(uuid) = Uuid::parse_str(id.as_str()) else { return };
        // Checa se tem config (variações/adicionais) lendo do snapshot.
        let (has_config, _name) = {
            let Ok(g) = pdv.lock() else { return };
            let Some(p) = g.products_all.iter().find(|p| p.base.id == uuid) else { return };
            let has_var = p.variations.as_deref()
                .map(|s| !s.trim().is_empty() && s.trim() != "[]")
                .unwrap_or(false);
            let has_addons = !p.addon_group_ids.is_empty();
            (has_var || has_addons, p.name.clone())
        };
        if !has_config {
            add_to_cart_simple(&pdv, uuid);
            if let Some(ui) = ui_weak.upgrade() {
                apply_state_to_ui(&ui, &pdv);
            }
            return;
        }
        // Abre o ProductConfiguratorModal no contexto PDV.
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_config_context(SharedString::from("pdv"));
            // Reaproveita o `start-product-config` que carrega
            // variações/adicionais e popula `config-*` no MainWindow.
            ui.invoke_start_product_config(SharedString::from(uuid.to_string()));
        }
        let _ = (state.clone(), handle.clone());
    });
}

/// Adiciona produto simples (sem config) no carrinho. Agrega na linha
/// existente quando já tem o mesmo produto e sem adicionais.
///
/// Usa `effective_unit_price` pra aplicar descontos (fixed/percent/bulk)
/// — o servidor faz a MESMA conta em `verify_item_prices` e rejeita
/// (Price mismatch) se o desktop mandar o preço bruto.
pub(crate) fn add_to_cart_simple(pdv: &Arc<Mutex<PdvState>>, product_id: Uuid) {
    let Ok(mut g) = pdv.lock() else { return };
    let product = match g.products_all.iter().find(|p| p.base.id == product_id) {
        Some(p) => p.clone(),
        None => return,
    };
    let price = letaf_core::discount::effective_unit_price(&product, 1.0);
    if let Some(existing) = g.cart.iter_mut().find(|i| {
        i.product_id == product_id && i.addons_json.is_none()
    }) {
        existing.qty += 1.0;
        return;
    }
    g.cart.push(CartItem {
        line_id: Uuid::new_v4(),
        product_id,
        name: product.name,
        qty: 1.0,
        unit_price: price,
        addons_summary: String::new(),
        addons_json: None,
    });
}

/// `pdv-config-confirm` — dispatchado pelo `ProductConfiguratorModal`
/// quando o operador clica "Adicionar" e `config-context == "pdv"`.
/// Lê o estado do configurador (qty, variações/adicionais selecionados)
/// e empurra um `CartItem` com `addons_json` montado.
pub(crate) fn setup_pdv_config_confirm(ui: &MainWindow, pdv: Arc<Mutex<PdvState>>) {
    let ui_weak = ui.as_weak();
    ui.on_pdv_config_confirm(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        let product_id_str = ui_ref.get_config_product_id().to_string();
        let product_name = ui_ref.get_config_product_name().to_string();
        let qty = ui_ref.get_config_qty().max(1) as f64;
        let base_price = ui_ref.get_config_base_price() as f64;
        // Monta snapshot dos addons selecionados — replica a lógica
        // do `setup_config_confirm` mas para o destino PDV.
        let variations = ui_ref.get_config_variations();
        let groups = ui_ref.get_config_addon_groups();
        let mut snapshot: Vec<serde_json::Value> = Vec::new();
        let mut extras = 0.0_f64;
        for vi in 0..slint_row_count(&variations) {
            let Some(v) = slint_row_data(&variations, vi) else { continue };
            let title = v.title.to_string();
            let opts = v.options;
            let selection = v.selection.to_string();
            if selection == "max_value" {
                let mut winner: Option<(String, f64)> = None;
                for oi in 0..slint_row_count(&opts) {
                    let Some(o) = slint_row_data(&opts, oi) else { continue };
                    if o.selected {
                        let p = o.price as f64;
                        if winner.as_ref().map(|(_, wp)| p > *wp).unwrap_or(true) {
                            winner = Some((o.name.to_string(), p));
                        }
                    }
                }
                if let Some((n, p)) = winner {
                    extras += p;
                    snapshot.push(serde_json::json!({ "group": title.clone(), "name": n, "price": p }));
                }
            } else {
                for oi in 0..slint_row_count(&opts) {
                    let Some(o) = slint_row_data(&opts, oi) else { continue };
                    if o.selected {
                        let p = o.price as f64;
                        extras += p;
                        snapshot.push(serde_json::json!({ "group": title.clone(), "name": o.name.to_string(), "price": p }));
                    }
                }
            }
        }
        for gi in 0..slint_row_count(&groups) {
            let Some(g) = slint_row_data(&groups, gi) else { continue };
            let group_name = g.name.to_string();
            let addons = g.addons;
            for ai in 0..slint_row_count(&addons) {
                let Some(a) = slint_row_data(&addons, ai) else { continue };
                if a.selected {
                    let p = a.price as f64;
                    extras += p;
                    snapshot.push(serde_json::json!({ "group": group_name.clone(), "name": a.name.to_string(), "price": p }));
                }
            }
        }
        let unit = base_price + extras;
        let (addons_json, addons_summary) = if snapshot.is_empty() {
            (None, String::new())
        } else {
            let json_str = serde_json::to_string(&serde_json::Value::Array(snapshot.clone()))
                .unwrap_or_default();
            // Resumo agrupado por grupo (mesmo formato do detalhe).
            let mut groups_label: Vec<(String, Vec<String>)> = Vec::new();
            for v in &snapshot {
                let Some(name) = v.get("name").and_then(|n| n.as_str()) else { continue };
                let group = v.get("group").and_then(|g| g.as_str()).unwrap_or("").to_string();
                match groups_label.iter_mut().find(|(k, _)| k == &group) {
                    Some((_, names)) => names.push(name.to_string()),
                    None => groups_label.push((group, vec![name.to_string()])),
                }
            }
            let summary = groups_label.into_iter()
                .map(|(g, names)| if g.is_empty() { names.join(", ") } else { format!("{g}: {}", names.join(", ")) })
                .collect::<Vec<_>>()
                .join(" · ");
            (Some(json_str), summary)
        };
        // Empurra no carrinho.
        let product_id = match Uuid::parse_str(&product_id_str) {
            Ok(u) => u, Err(_) => return,
        };
        if let Ok(mut g) = pdv.lock() {
            g.cart.push(CartItem {
                line_id: Uuid::new_v4(),
                product_id,
                name: product_name,
                qty,
                unit_price: unit,
                addons_summary,
                addons_json,
            });
        }
        // Reseta o configurador (cancel limpa tudo + edit_idx=-1).
        ui_ref.invoke_config_cancel();
        ui_ref.set_config_context(SharedString::from("edit_order"));
        apply_state_to_ui(&ui_ref, &pdv);
    });
}

