use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use tokio::sync::Notify;
use uuid::Uuid;

use letaf_core::product::model::Product;

use crate::context::DesktopState;
use crate::{CategoryFilterEntry, MainWindow, ProductData, SubcategoryFilterEntry};

use super::super::helpers::show_toast;
use super::super::image::decode_single_product_image;
use super::state::{DecodedProduct, ProductFilterState, SharedFilter};
use super::data::{addon_group_ids_to_csv, build_product_data_from_product, decoded_to_product_data_ref, make_product_display, parse_hex_color, push_product_to_model, to_decoded_product};

/// Callback: carrega todos os produtos da empresa.
pub(crate) fn setup_refresh(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    cache: Arc<std::sync::Mutex<Vec<DecodedProduct>>>,
    filter: SharedFilter,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_refresh_products(move || {
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let cache = cache.clone();
        let filter = filter.clone();

        handle.spawn(async move {
            let company_id = state.company_id();

            let (products_result, cats_result, subs_result) = tokio::join!(
                state.product_service.find_all(company_id),
                state.category_service.find_all(company_id),
                state.subcategory_service.find_all(company_id),
            );

            let products = match products_result {
                Ok(p) => p,
                Err(e) => {
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        ui.set_status_message(SharedString::from(format!("Erro: {e}")));
                    });
                    return;
                }
            };

            // Lista plana e ordenada de categorias/subcategorias para o painel
            // de filtros. Ordena por nome para estabilidade visual.
            let mut categories_vec: Vec<(String, String)> = cats_result
                .unwrap_or_default()
                .into_iter()
                .map(|c| (c.base.id.to_string(), c.name))
                .collect();
            categories_vec.sort_by_key(|a| a.1.to_lowercase());
            let cat_map: HashMap<String, String> =
                categories_vec.iter().cloned().collect();

            let mut subcategories_vec: Vec<(String, String, String)> = subs_result
                .unwrap_or_default()
                .into_iter()
                .map(|s| (
                    s.base.id.to_string(),
                    s.category_id.to_string(),
                    s.name,
                ))
                .collect();
            subcategories_vec.sort_by_key(|a| a.2.to_lowercase());
            let sub_map: HashMap<String, String> = subcategories_vec.iter()
                .map(|(id, _, name)| (id.clone(), name.clone())).collect();

            let count = products.len();

            // Decodifica base64 → pixels brutos em thread pool (CPU-bound).
            // slint::Image não é Send, mas SharedPixelBuffer<Rgba8Pixel> é Send.
            let decoded = tokio::task::spawn_blocking(move || {
                products.iter()
                    .map(|p| to_decoded_product(p, &cat_map, &sub_map))
                    .collect::<Vec<_>>()
            })
            .await
            .unwrap_or_default();

            // Atualiza cache + estado de filtro antes de entrar no event loop.
            if let Ok(mut g) = cache.lock() { *g = decoded; }
            if let Ok(mut f) = filter.lock() {
                let valid_cats: HashSet<String> =
                    categories_vec.iter().map(|(id, _)| id.clone()).collect();
                let valid_subs: HashSet<String> =
                    subcategories_vec.iter().map(|(id, _, _)| id.clone()).collect();
                f.known_categories = categories_vec;
                f.known_subcategories = subcategories_vec;
                // Limpa seleções para itens que não existem mais.
                f.selected_categories.retain(|id| valid_cats.contains(id));
                f.selected_subcategories.retain(|id| valid_subs.contains(id));
            }

            // Re-aplica filtros no event loop (contagens + grade).
            let cache2 = cache.clone();
            let filter2 = filter.clone();
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                // Contagens absolutas (sobre cache total, não filtrado).
                let (active, inactive) = cache2.lock().map(|g| {
                    let a = g.iter().filter(|p| p.active).count() as i32;
                    let i = g.len() as i32 - a;
                    (a, i)
                }).unwrap_or((0, 0));
                ui.set_products_active_count(active);
                ui.set_products_inactive_count(inactive);
                refresh_products_view(&ui, &cache2, &filter2);
                // Re-aplica a seleção atual (mantém detalhe sincronizado
                // com a versão recém-carregada; limpa quando o produto
                // não está mais no cache).
                let cur_id = ui.get_selected_product_id().to_string();
                apply_selected_product(&ui, &cache2, &cur_id);
                ui.set_status_message(SharedString::from(
                    format!("{count} produto(s) carregado(s)"),
                ));
            });
        });
    });
}

/// Aplica todos os filtros (busca textual, categorias, subcategorias,
/// status, estoque) sobre o cache local e atualiza a UI.
///
/// Regras aplicadas (AI_RULES.md §1, §8):
/// - Função pura sobre o cache (sem efeitos de rede/BD).
/// - Conta `filter-active-count` para mostrar badge no botão Filtros:
///   conta como ativo cada filtro que difere do padrão "vendável".
pub(crate) fn refresh_products_view(
    ui: &MainWindow,
    cache: &Arc<std::sync::Mutex<Vec<DecodedProduct>>>,
    filter: &SharedFilter,
) {
    let (data, cats, subs, badge_count) = {
        let f = filter.lock().expect("filter mutex poisoned");
        let cache_guard = cache.lock().expect("cache mutex poisoned");
        let data: Vec<ProductData> = cache_guard
            .iter()
            .filter(|p| filter_matches(p, &f))
            .map(decoded_to_product_data_ref)
            .collect();
        let cats: Vec<CategoryFilterEntry> = f.known_categories.iter().map(|(id, name)| {
            CategoryFilterEntry {
                id: SharedString::from(id.as_str()),
                name: SharedString::from(name.as_str()),
                selected: f.selected_categories.contains(id),
            }
        }).collect();
        let subs: Vec<SubcategoryFilterEntry> = f.known_subcategories.iter()
            .filter(|(_, cat_id, _)| {
                // Se há categorias filtradas, só mostra subcategorias dessas;
                // caso contrário, lista todas conhecidas.
                f.selected_categories.is_empty() || f.selected_categories.contains(cat_id)
            })
            .map(|(id, cat_id, name)| SubcategoryFilterEntry {
                id: SharedString::from(id.as_str()),
                category_id: SharedString::from(cat_id.as_str()),
                name: SharedString::from(name.as_str()),
                selected: f.selected_subcategories.contains(id),
            }).collect();
        let mut badge = 0;
        if !f.selected_categories.is_empty()    { badge += 1; }
        if !f.selected_subcategories.is_empty() { badge += 1; }
        if f.status != "active" { badge += 1; }
        if f.stock  != "with"   { badge += 1; }
        (data, cats, subs, badge)
    };
    ui.set_products(ModelRc::new(VecModel::from(data)));
    ui.set_filter_cats(ModelRc::new(VecModel::from(cats)));
    ui.set_filter_subs(ModelRc::new(VecModel::from(subs)));
    ui.set_filter_active_count(badge_count);
}

/// Atualiza apenas `selected-product-id` + `detail-product` no cache.
///
/// Usado pelo `setup_refresh` (re-aplica seleção após reload sem
/// reescrever o formulário em andamento) e pelo `setup_update_product`
/// (após salvar, refresca o snapshot do cartão à direita).  O sentinel
/// `"new"` é preservado (o painel direito decide pelo `editing-id == ""`).
pub(crate) fn apply_selected_product(
    ui: &MainWindow,
    cache: &std::sync::Mutex<Vec<DecodedProduct>>,
    id: &str,
) {
    if id.is_empty() || id == "new" {
        return;
    }
    let found = cache.lock().ok().and_then(|g| {
        g.iter().find(|p| p.id == id).map(decoded_to_product_data_ref)
    });
    if let Some(data) = found {
        ui.set_selected_product_id(SharedString::from(id));
        ui.set_detail_product(data);
    } else {
        // Produto não existe mais (filtrado/deletado) → solta a seleção.
        ui.set_selected_product_id(SharedString::default());
        ui.set_detail_product(ProductData::default());
    }
}

/// Preenche todos os campos do formulário a partir do `DecodedProduct`.
///
/// Antes morava no `request-edit(p)` dentro do `main.slint`; centralizar
/// em Rust elimina a lógica do .slint (AI_RULES §1) e permite chamar do
/// `setup_select_product` (clique inline no master-detail).
fn fill_form_from_decoded(ui: &MainWindow, d: &DecodedProduct) {
    ui.set_editing_id(d.id.clone());
    ui.set_product_name(d.name.clone());
    ui.set_product_description(d.description.clone());
    ui.set_product_price(d.price.clone());
    ui.set_product_cost_price(d.cost_price.clone());
    ui.set_product_stock_quantity(d.stock_quantity.clone());
    ui.set_product_min_stock(d.min_stock.clone());
    ui.set_product_margin_display(d.margin_pct_display.clone());
    ui.set_product_unlimited_stock(d.unlimited_stock);
    let availability_enabled = !d.availability_schedule.is_empty();
    ui.set_product_availability_enabled(availability_enabled);
    ui.invoke_load_product_availability(d.availability_schedule.clone());
    let kind = if d.discount_kind.is_empty() {
        SharedString::from("none")
    } else {
        d.discount_kind.clone()
    };
    ui.set_product_discount_kind(kind);
    ui.set_product_discount_value(d.discount_value.clone());
    ui.set_product_discount_min_qty(d.discount_min_qty.clone());
    ui.invoke_load_discount_tiers(d.discount_tiers.clone());
    ui.invoke_load_product_addon_groups(d.addon_group_ids.clone());
    ui.invoke_load_product_variations(d.variations.clone());
    ui.set_product_barcode(d.barcode.clone());
    ui.set_product_unit(d.unit.clone());
    ui.set_product_balance_mode(d.balance_mode.clone());
    ui.set_product_image_data(d.image_data.clone());
    ui.set_product_cover_color(d.cover_color.clone());
    ui.set_product_category_id(d.category_id.clone());
    ui.set_product_category_name(d.category_name.clone());
    ui.set_product_subcategory_id(d.subcategory_id.clone());
    ui.set_product_subcategory_name(d.subcategory_name.clone());
    ui.set_product_category_open(false);
    ui.set_product_subcategory_open(false);
    ui.set_product_error_name(SharedString::default());
    ui.set_product_error_price(SharedString::default());
    ui.set_product_error_stock(SharedString::default());
    ui.set_product_save_error(SharedString::default());
}

/// Callback: zera o `detail-product` (snapshot do painel direito).
///
/// Disparado pelo Slint no `request-new` (e variantes) antes de zerar
/// o form. Slint não permite atribuir campos individuais de struct, e
/// o cabeçalho do master-detail lê de `detail-product.product-image`
/// (entre outros) — sem este reset, o card de imagem ficava mostrando
/// a foto do último produto selecionado mesmo em modo "Novo".
pub(crate) fn setup_clear_detail_product(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_clear_detail_product(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        ui.set_detail_product(ProductData::default());
    });
}

/// Callback: clique numa linha da lista mestra → preenche o form do
/// painel direito e atualiza o snapshot dos cartões.
pub(crate) fn setup_select_product(
    ui: &MainWindow,
    cache: Arc<std::sync::Mutex<Vec<DecodedProduct>>>,
) {
    let ui_weak = ui.as_weak();
    ui.on_select_product(move |id| {
        let Some(ui) = ui_weak.upgrade() else { return };
        let id = id.as_str();
        if id.is_empty() || id == "new" {
            ui.set_selected_product_id(SharedString::from(id));
            return;
        }
        // Lê DecodedProduct do cache, preenche form e cartão direito.
        let snapshot = cache.lock().ok().and_then(|g| {
            g.iter().find(|p| p.id.as_str() == id).map(|d| (
                decoded_to_product_data_ref(d),
                clone_decoded_basics(d),
            ))
        });
        if let Some((data, basics)) = snapshot {
            ui.set_selected_product_id(SharedString::from(id));
            ui.set_detail_product(data);
            fill_form_from_decoded(&ui, &basics);
        } else {
            ui.set_selected_product_id(SharedString::default());
            ui.set_detail_product(ProductData::default());
        }
    });
}

/// Constrói um `DecodedProduct` a partir de um `Product` recém-salvo,
/// com nomes já resolvidos e pixel buffer (já decodificado). Espelha
/// `to_decoded_product` mas dispensa as `HashMap`s — caller pós-CRUD
/// já tem os nomes em mãos.
pub(crate) fn decoded_from_components(
    p: &Product,
    cat_name: &str,
    sub_name: &str,
    pixel_buf: Option<slint::SharedPixelBuffer<slint::Rgba8Pixel>>,
) -> DecodedProduct {
    let cat_id = p.category_id.map(|u| u.to_string()).unwrap_or_default();
    let sub_id = p.subcategory_id.map(|u| u.to_string()).unwrap_or_default();
    let cover_color = p.cover_color.clone().unwrap_or_default();
    let cover_color_rgb = parse_hex_color(&cover_color);
    let disp = make_product_display(p);
    DecodedProduct {
        id: SharedString::from(p.base.id.to_string()),
        name: SharedString::from(p.name.as_str()),
        description: SharedString::from(p.description.as_deref().unwrap_or("")),
        price: disp.price,
        price_display: disp.price_display,
        cost_price: disp.cost_price,
        cost_price_display: disp.cost_price_display,
        margin_amount_display: disp.margin_amount_display,
        margin_pct_display: disp.margin_pct_display,
        stock_quantity: disp.stock_quantity,
        stock_status: disp.stock_status,
        stock_status_label: disp.stock_status_label,
        min_stock: disp.min_stock,
        min_stock_display: disp.min_stock_display,
        purchase_suggestion: disp.purchase_suggestion,
        unlimited_stock: p.unlimited_stock,
        barcode: SharedString::from(p.barcode.as_deref().unwrap_or("")),
        unit: SharedString::from(p.unit.as_str()),
        active: p.active,
        web_visible: p.web_visible,
        synced: disp.synced,
        sync_label: disp.sync_label,
        balance_mode: SharedString::from(p.balance_mode.as_db_str()),
        image_data: SharedString::from(p.image_data.as_deref().unwrap_or("")),
        category_id: SharedString::from(cat_id),
        category_name: SharedString::from(cat_name),
        subcategory_id: SharedString::from(sub_id),
        subcategory_name: SharedString::from(sub_name),
        cover_color: SharedString::from(cover_color),
        cover_color_rgb,
        availability_schedule: SharedString::from(p.availability_schedule.as_deref().unwrap_or("")),
        discount_kind: SharedString::from(p.discount_kind.as_deref().unwrap_or("")),
        discount_value: SharedString::from(
            p.discount_value.map(|v| format!("{v}")).unwrap_or_default()
        ),
        discount_min_qty: SharedString::from(
            p.discount_min_qty.map(|v| format!("{v}")).unwrap_or_default()
        ),
        discount_tiers: SharedString::from(p.discount_tiers.as_deref().unwrap_or("")),
        addon_group_ids: SharedString::from(addon_group_ids_to_csv(&p.addon_group_ids)),
        variations: SharedString::from(p.variations.as_deref().unwrap_or("")),
        pixel_buffer: pixel_buf,
    }
}

/// Upsert no cache local. Substitui entrada existente (mesmo ID) ou
/// adiciona ao final. Essencial pós-CRUD: sem isso, `setup_select_product`
/// (que lê do cache) repopularia o form com a versão ANTERIOR à edição
/// — bug observado quando trocava a imagem, saía e voltava no produto,
/// e a imagem antiga reaparecia.
pub(crate) fn upsert_decoded_in_cache(
    cache: &Arc<std::sync::Mutex<Vec<DecodedProduct>>>,
    decoded: DecodedProduct,
) {
    if let Ok(mut g) = cache.lock() {
        if let Some(pos) = g.iter().position(|d| d.id == decoded.id) {
            g[pos] = decoded;
        } else {
            // Novos produtos vão para o TOPO (ordem `created_at DESC`
            // do `find_all`). Sem isso, o duplicado/recém-criado ficava
            // no rodapé do cache e ao próximo refresh saltaria pro topo —
            // inconsistência visual.
            g.insert(0, decoded);
        }
    }
}

/// Remove uma entrada do cache local pelo ID.
pub(crate) fn remove_from_cache(
    cache: &Arc<std::sync::Mutex<Vec<DecodedProduct>>>,
    id: &str,
) {
    if let Ok(mut g) = cache.lock() {
        g.retain(|d| d.id.as_str() != id);
    }
}

/// Clone "barato" dos campos textuais do DecodedProduct (sem o
/// pixel buffer pesado) — usado por `fill_form_from_decoded` fora do
/// lock do cache.
fn clone_decoded_basics(d: &DecodedProduct) -> DecodedProduct {
    DecodedProduct {
        id: d.id.clone(),
        name: d.name.clone(),
        description: d.description.clone(),
        price: d.price.clone(),
        price_display: d.price_display.clone(),
        cost_price: d.cost_price.clone(),
        cost_price_display: d.cost_price_display.clone(),
        margin_amount_display: d.margin_amount_display.clone(),
        margin_pct_display: d.margin_pct_display.clone(),
        stock_quantity: d.stock_quantity.clone(),
        stock_status: d.stock_status.clone(),
        stock_status_label: d.stock_status_label.clone(),
        min_stock: d.min_stock.clone(),
        min_stock_display: d.min_stock_display.clone(),
        purchase_suggestion: d.purchase_suggestion.clone(),
        unlimited_stock: d.unlimited_stock,
        barcode: d.barcode.clone(),
        unit: d.unit.clone(),
        active: d.active,
        web_visible: d.web_visible,
        synced: d.synced,
        sync_label: d.sync_label.clone(),
        balance_mode: d.balance_mode.clone(),
        image_data: d.image_data.clone(),
        category_id: d.category_id.clone(),
        category_name: d.category_name.clone(),
        subcategory_id: d.subcategory_id.clone(),
        subcategory_name: d.subcategory_name.clone(),
        cover_color: d.cover_color.clone(),
        cover_color_rgb: d.cover_color_rgb,
        availability_schedule: d.availability_schedule.clone(),
        discount_kind: d.discount_kind.clone(),
        discount_value: d.discount_value.clone(),
        discount_min_qty: d.discount_min_qty.clone(),
        discount_tiers: d.discount_tiers.clone(),
        addon_group_ids: d.addon_group_ids.clone(),
        variations: d.variations.clone(),
        pixel_buffer: d.pixel_buffer.clone(),
    }
}

/// Callback: "Duplicar" no header do detalhe.
///
/// Regras aplicadas (AI_RULES.md §1, §7, §11):
/// - Lê o original via `product_service.find_by_id`.
/// - Constrói um novo registro via `product_service.create` (com nome
///   "<original> (cópia)"). O service valida tudo, então duplicar
///   herda o mesmo crivo do "Novo Produto".
/// - Após sucesso: dispara sync, recarrega a lista e seleciona o novo
///   produto no painel direito.
pub(crate) fn setup_duplicate_product(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
    cache: Arc<std::sync::Mutex<Vec<DecodedProduct>>>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_duplicate_product(move |id_str| {
        let id = match Uuid::parse_str(id_str.as_str()) {
            Ok(id) => id,
            Err(e) => {
                tracing::error!("Invalid product ID for duplicate: {e}");
                return;
            }
        };
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let notify = sync_notify.clone();
        let cache = cache.clone();
        // Lê os nomes de categoria/subcategoria do original a partir
        // do cache (mesma empresa, JÁ está em memória) — evita uma
        // ida extra ao banco. Vazio se o original não estiver no cache.
        let (cat_name, sub_name) = {
            let original_id = id.to_string();
            if let Ok(g) = cache.lock() {
                g.iter().find(|d| d.id.as_str() == original_id)
                    .map(|d| (d.category_name.to_string(), d.subcategory_name.to_string()))
                    .unwrap_or_default()
            } else {
                (String::new(), String::new())
            }
        };
        handle.spawn(async move {
            let cid = state.company_id();
            let original = match state.product_service.find_by_id(cid, id).await {
                Ok(Some(p)) => p,
                Ok(None) => {
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        show_toast(&ui, "Produto não encontrado", "error");
                    });
                    return;
                }
                Err(e) => {
                    let msg = format!("Erro ao ler produto: {e}");
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        show_toast(&ui, &msg, "error");
                    });
                    return;
                }
            };
            let new_name = format!("{} (cópia)", original.name);
            let result = state.product_service.create(
                cid,
                new_name.clone(),
                original.description.clone(),
                original.category_id,
                original.subcategory_id,
                original.price,
                original.cost_price,
                original.stock_quantity,
                original.min_stock,
                original.unlimited_stock,
                original.barcode.clone(),
                original.unit.clone(),
                original.balance_mode,
                original.image_data.clone(),
                original.cover_color.clone(),
                original.availability_schedule.clone(),
                original.discount_kind.clone(),
                original.discount_value,
                original.discount_min_qty,
                original.discount_tiers.clone(),
                original.addon_group_ids.clone(),
                original.variations.clone(),
            ).await;

            if result.is_ok() { notify.notify_one(); }
            match result {
                Ok(p) => {
                    // Decodifica a imagem (mesma do original) UMA vez
                    // e reaproveita em cache + detail + form.
                    let pixel_buf = decode_single_product_image(p.image_data.clone()).await;
                    let p_name = p.name.clone();
                    let new_id = SharedString::from(p.base.id.to_string());
                    // Cache + form lêem dos mesmos componentes — usa o
                    // mesmo helper que o save/add.
                    let decoded = decoded_from_components(&p, &cat_name, &sub_name, pixel_buf.clone());
                    let decoded_for_form = clone_decoded_basics(&decoded);
                    upsert_decoded_in_cache(&cache, decoded);
                    // `ProductData` contém `slint::Image` (não Send) —
                    // construir DENTRO do event loop (mesmo padrão de
                    // `setup_add`). `p`, `pixel_buf` e `cat_name`/`sub_name`
                    // são todos Send.
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        let p_data = build_product_data_from_product(&p, &cat_name, &sub_name, pixel_buf);
                        push_product_to_model(&ui, p_data.clone());
                        ui.set_selected_product_id(new_id.clone());
                        ui.set_detail_product(p_data);
                        // Preenche todos os campos do form com a cópia
                        // — sem isso, "Nome" e demais inputs ficavam
                        // presos no original e um Save sobrescreveria.
                        fill_form_from_decoded(&ui, &decoded_for_form);
                        show_toast(&ui, &format!("Produto '{}' duplicado", p_name), "success");
                        ui.set_status_message(SharedString::from(format!("Produto '{}' duplicado", p_name)));
                    });
                }
                Err(e) => {
                    let msg = format!("Erro ao duplicar: {e}");
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        show_toast(&ui, &msg, "error");
                    });
                }
            }
        });
    });
}

/// Avalia se o produto decodificado passa por todos os filtros ativos.
fn filter_matches(p: &DecodedProduct, f: &ProductFilterState) -> bool {
    // Categoria: vazio = sem restrição; senão, ID deve estar marcado.
    if !f.selected_categories.is_empty()
        && !f.selected_categories.contains(p.category_id.as_str())
    {
        return false;
    }
    // Subcategoria idem (uma vazia também não restringe).
    if !f.selected_subcategories.is_empty()
        && !f.selected_subcategories.contains(p.subcategory_id.as_str())
    {
        return false;
    }
    // Status
    let status_ok = match f.status.as_str() {
        "active"   => p.active,
        "inactive" => !p.active,
        _          => true, // "both"
    };
    if !status_ok { return false; }
    // Estoque (compara a string formatada — "Sem Estoque" → out)
    let has_stock = p.stock_status.as_str() != "out";
    let stock_ok = match f.stock.as_str() {
        "with"    => has_stock,
        "without" => !has_stock,
        _         => true, // "both"
    };
    if !stock_ok { return false; }
    // Busca textual em vários campos.
    let q = f.search_query.to_lowercase();
    if q.is_empty() { return true; }
    p.name.to_lowercase().contains(&q)
        || p.description.to_lowercase().contains(&q)
        || p.barcode.to_lowercase().contains(&q)
        || p.category_name.to_lowercase().contains(&q)
        || p.subcategory_name.to_lowercase().contains(&q)
}

