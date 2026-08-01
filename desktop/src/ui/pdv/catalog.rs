use std::collections::HashMap;
use rust_decimal::prelude::ToPrimitive;
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
use crate::{PdvUiState, ProductConfigState};

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
    setup_split_amount_changed(ui, pdv.clone());
    setup_recalc(ui, pdv);
}

pub(crate) fn setup_recalc(ui: &MainWindow, pdv: Arc<Mutex<PdvState>>) {
    let ui_weak = ui.as_weak();
    ui.global::<PdvUiState>().on_pdv_recalc(move || {
        if let Some(ui) = ui_weak.upgrade() {
            apply_state_to_ui(&ui, &pdv);
        }
    });
}

pub(crate) fn setup_discount_changed(ui: &MainWindow, pdv: Arc<Mutex<PdvState>>) {
    let ui_weak = ui.as_weak();
    ui.global::<PdvUiState>().on_pdv_discount_changed(move |raw| {
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
    ui.global::<PdvUiState>().on_pdv_additional_changed(move |raw| {
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
    ui.global::<PdvUiState>().on_pdv_amount_paid_changed(move |raw| {
        if let Ok(mut g) = pdv.lock() {
            g.amount_paid = parse_amount(raw.as_str());
        }
        if let Some(ui) = ui_weak.upgrade() {
            apply_state_to_ui(&ui, &pdv);
        }
    });
}

/// Parse do valor de cada linha do rateio (pagamento parcial). Guarda o
/// valor parseado na prop float correspondente e recalcula o restante
/// (feito em `apply_state_to_ui`). Só o parse string→float vive no Rust;
/// a revelação progressiva das próximas linhas é reativa no Slint.
pub(crate) fn setup_split_amount_changed(ui: &MainWindow, pdv: Arc<Mutex<PdvState>>) {
    let ui_weak = ui.as_weak();
    ui.global::<PdvUiState>().on_pdv_split_amount_changed(move |line, raw| {
        let Some(ui) = ui_weak.upgrade() else { return };
        let value = parse_amount(raw.as_str()) as f32;
        match line {
            0 => ui.global::<PdvUiState>().set_pdv_split_v1(value),
            1 => ui.global::<PdvUiState>().set_pdv_split_v2(value),
            2 => ui.global::<PdvUiState>().set_pdv_split_v3(value),
            _ => {}
        }
        apply_state_to_ui(&ui, &pdv);
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
    ui.global::<PdvUiState>().on_pdv_refresh(move || {
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let pdv = pdv.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            let products = state.product_service.find_all(cid).await.unwrap_or_default();
            let categories = state.category_service.find_all(cid).await.unwrap_or_default();
            let customers = state.customer_service.find_all(cid).await.unwrap_or_default();
            // Taxa de entrega configurada na empresa (fonte de verdade).
            let delivery_fee = state.company_service.find_by_id(cid).await.ok().flatten()
                .and_then(|c| c.delivery_fee.to_f64())
                .unwrap_or(0.0);
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
                g.delivery_fee = delivery_fee;
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
    ui.global::<PdvUiState>().on_pdv_search_changed(move |q| {
        let trimmed = q.trim().to_string();
        // Auto-add por barcode. Duas formas:
        // 1) match EXATO do `barcode` cadastrado (produto unitário);
        // 2) EAN-13 emitido por BALANÇA (prefixo 2): os 6 dígitos do meio
        //    são o `barcode` do produto e os 5 seguintes o peso/preço.
        //    O parser já existia em `core::product::balance` e nunca era
        //    chamado — o código da balança nunca casava e o item
        //    simplesmente não entrava no carrinho.
        let (matched_pid, balanca) = {
            let g = match pdv.lock() { Ok(g) => g, Err(_) => return };
            if trimmed.is_empty() {
                (None, None)
            } else if let Some(exato) = g
                .products_all
                .iter()
                .find(|p| p.barcode.as_deref() == Some(trimmed.as_str()))
            {
                (Some(exato.base.id), None)
            } else if let Some(code) = letaf_core::product::balance::parse_balance_barcode(&trimmed)
            {
                let achado = g
                    .products_all
                    .iter()
                    .find(|p| p.barcode.as_deref() == Some(code.product_code.as_str()));
                match achado {
                    Some(p) => (Some(p.base.id), Some((code.raw_value, p.balance_mode))),
                    None => (None, None),
                }
            } else {
                (None, None)
            }
        };
        if let Some(pid) = matched_pid {
            match balanca {
                Some((raw, modo)) => add_balance_item(&pdv, pid, raw, modo),
                None => add_to_cart_simple(&pdv, pid),
            }
            if let Some(ui) = ui_weak.upgrade() {
                ui.global::<PdvUiState>().set_pdv_search_text(SharedString::default());
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
    ui.global::<PdvUiState>().on_pdv_submit_search(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        let q = ui_ref.global::<PdvUiState>().get_pdv_search_text().to_string();
        let trimmed = q.trim().to_string();
        if trimmed.is_empty() { return; }
        let matched = pdv.lock().ok().and_then(|g| {
            g.products_all.iter()
                .find(|p| p.barcode.as_deref().map(|b| b == trimmed).unwrap_or(false))
                .map(|p| p.base.id)
        });
        if let Some(pid) = matched {
            add_to_cart_simple(&pdv, pid);
            ui_ref.global::<PdvUiState>().set_pdv_search_text(SharedString::default());
            if let Ok(mut g) = pdv.lock() { g.search_query.clear(); }
            apply_state_to_ui(&ui_ref, &pdv);
        }
    });
}

pub(crate) fn setup_toggle_category(ui: &MainWindow, pdv: Arc<Mutex<PdvState>>) {
    let ui_weak = ui.as_weak();
    ui.global::<PdvUiState>().on_pdv_toggle_category(move |id| {
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
    ui.global::<PdvUiState>().on_pdv_clear_categories(move || {
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
    ui.global::<PdvUiState>().on_pdv_cats_width_changed(move |w| {
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
    ui.global::<PdvUiState>().on_pdv_add_product(move |id| {
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
            ui.global::<ProductConfigState>().set_config_context(SharedString::from("pdv"));
            // Reaproveita o `start-product-config` que carrega
            // variações/adicionais e popula `config-*` no MainWindow.
            ui.global::<ProductConfigState>().invoke_start_product_config(SharedString::from(uuid.to_string()));
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
    // `products_all` fica emprestado dentro do bloco — clona a lista de
    // referência do reprice fora do iter_mut para satisfazer o borrow.
    if let Some(idx) = g.cart.iter().position(|i| {
        i.product_id == product_id && i.addons_json.is_none()
    }) {
        g.cart[idx].qty += 1.0;
        // Reprecifica: descontos `bulk_*` mudam o unitário conforme a
        // qty — o core recomputa igual em `verify_item_prices`.
        let mut line = g.cart[idx].clone();
        line.reprice(&g.products_all);
        g.cart[idx] = line;
        return;
    }
    g.cart.push(CartItem {
        line_id: Uuid::new_v4(),
        product_id,
        name: product.name,
        qty: 1.0,
        unit_price: price.to_f64().unwrap_or(0.0),
        extras: 0.0,
        addons_summary: String::new(),
        addons_json: None,
    });
}

/// Adiciona ao carrinho um item lido da BALANÇA.
///
/// `raw` é a janela variável do EAN-13, cuja semântica vem do cadastro
/// (`Product.balance_mode`):
/// - `Weight`: gramas — a quantidade é `raw/1000` kg pelo preço por kg;
/// - `Price`: centavos — o total já está no código, então a linha entra
///   com quantidade 1 e o unitário sendo o valor lido.
///
/// Cada leitura vira uma LINHA nova (não soma na existente): duas pesagens
/// do mesmo produto são itens distintos, com pesos distintos.
pub(crate) fn add_balance_item(
    pdv: &Arc<Mutex<PdvState>>,
    product_id: Uuid,
    raw: u32,
    modo: letaf_core::product::model::BalanceMode,
) {
    use letaf_core::product::model::BalanceMode;
    let Ok(mut g) = pdv.lock() else { return };
    let product = match g.products_all.iter().find(|p| p.base.id == product_id) {
        Some(p) => p.clone(),
        None => return,
    };
    let (qty, unit) = match modo {
        BalanceMode::Weight => {
            let kg = f64::from(raw) / 1000.0;
            let por_kg = letaf_core::discount::effective_unit_price(&product, kg)
                .to_f64()
                .unwrap_or(0.0);
            (kg, por_kg)
        }
        // Preço em centavos: o total veio pronto da balança.
        BalanceMode::Price => (1.0, f64::from(raw) / 100.0),
    };
    if qty <= 0.0 {
        return;
    }
    g.cart.push(CartItem {
        line_id: Uuid::new_v4(),
        product_id,
        name: product.name,
        qty,
        unit_price: unit,
        extras: 0.0,
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
    ui.global::<PdvUiState>().on_pdv_config_confirm(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        let product_id_str = ui_ref.global::<ProductConfigState>().get_config_product_id().to_string();
        let product_name = ui_ref.global::<ProductConfigState>().get_config_product_name().to_string();
        let qty = ui_ref.global::<ProductConfigState>().get_config_qty().max(1) as f64;
        let base_price = ui_ref.global::<ProductConfigState>().get_config_base_price() as f64;
        // Monta snapshot dos addons selecionados — replica a lógica
        // do `setup_config_confirm` mas para o destino PDV.
        let variations = ui_ref.global::<ProductConfigState>().get_config_variations();
        let groups = ui_ref.global::<ProductConfigState>().get_config_addon_groups();
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
                    snapshot.push(serde_json::json!({ "group": title.clone(), "name": n, "price": letaf_core::money::price_to_json_string(letaf_core::money::from_db_f64(p)) }));
                }
            } else {
                for oi in 0..slint_row_count(&opts) {
                    let Some(o) = slint_row_data(&opts, oi) else { continue };
                    if o.selected {
                        let p = o.price as f64;
                        extras += p;
                        snapshot.push(serde_json::json!({ "group": title.clone(), "name": o.name.to_string(), "price": letaf_core::money::price_to_json_string(letaf_core::money::from_db_f64(p)) }));
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
                    snapshot.push(serde_json::json!({ "group": group_name.clone(), "name": a.name.to_string(), "price": letaf_core::money::price_to_json_string(letaf_core::money::from_db_f64(p)) }));
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
            let mut line = CartItem {
                line_id: Uuid::new_v4(),
                product_id,
                name: product_name,
                qty,
                unit_price: unit,
                extras,
                addons_summary,
                addons_json,
            };
            // `config-base-price` foi calculado com qty=1; reprecifica
            // para a qty escolhida (descontos `bulk_*`).
            line.reprice(&g.products_all);
            g.cart.push(line);
        }
        // Reseta o configurador (cancel limpa tudo + edit_idx=-1).
        ui_ref.global::<ProductConfigState>().invoke_config_cancel();
        ui_ref.global::<ProductConfigState>().set_config_context(SharedString::from("edit_order"));
        apply_state_to_ui(&ui_ref, &pdv);
    });
}

