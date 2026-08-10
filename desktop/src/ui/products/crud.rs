use std::sync::Arc;

use slint::{ComponentHandle, Model, SharedString};
use tokio::sync::Notify;
use uuid::Uuid;

use letaf_core::product::model::ProductImage;

use crate::context::DesktopState;
use crate::MainWindow;

use super::super::helpers::{friendly_error, show_toast};
use super::super::image::decode_single_product_image;
use super::state::DecodedProduct;
use super::list::{decoded_from_components, upsert_decoded_in_cache};
use super::form::{read_product_form, validate_product_form};
use super::data::{build_product_data_from_product, push_product_to_model, replace_product_in_model};
use crate::ProductsState;

/// Lê a galeria (imagens adicionais) do form → base64. Só a "loja" mostra o
/// card; para os demais tipos a lista vem vazia (nada a persistir).
fn read_gallery_images(ui: &MainWindow) -> Vec<ProductImage> {
    let model = ui.global::<ProductsState>().get_product_gallery();
    (0..model.row_count())
        .filter_map(|i| model.row_data(i))
        .map(|g| ProductImage { image_data: g.data.to_string() })
        .filter(|img| !img.image_data.is_empty())
        .collect()
}

/// Callback: PRODUZIR (recurso da "fábrica"). Produz a quantidade informada
/// do produto em edição — consome a ficha técnica e dá entrada no estoque
/// (backend `ProductService::produce`, atômico). Erros (qtd inválida, insumo
/// insuficiente) voltam para o modal.
pub(crate) fn setup_produce(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.global::<ProductsState>().on_produce_confirm(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        let id_str = ui.get_editing_id().to_string();
        let Ok(id) = Uuid::parse_str(&id_str) else { return };
        // Aceita vírgula (pt-BR). Validação forte fica no backend (§11).
        let qty: f64 = ui
            .global::<ProductsState>()
            .get_produce_quantity()
            .replace(',', ".")
            .trim()
            .parse()
            .unwrap_or(0.0);
        if qty <= 0.0 || !qty.is_finite() {
            ui.global::<ProductsState>().set_produce_error("Informe uma quantidade válida".into());
            return;
        }
        let ui_weak = ui.as_weak();
        let state = state.clone();
        let notify = sync_notify.clone();
        handle.spawn(async move {
            let result = state.product_service.produce(state.company_id(), id, qty).await;
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                match result {
                    Ok(()) => {
                        ui.global::<ProductsState>().set_produce_modal_open(false);
                        ui.global::<ProductsState>().set_produce_error(SharedString::default());
                        show_toast(&ui, &format!("Produzido: {qty}"), "success");
                        // Estoque mudou (produto + insumos) — recarrega a lista.
                        ui.global::<ProductsState>().invoke_refresh_products();
                    }
                    Err(e) => {
                        let msg = SharedString::from(friendly_error(&e));
                        ui.global::<ProductsState>().set_produce_error(msg.clone());
                        show_toast(&ui, msg.as_str(), "error");
                    }
                }
            });
            notify.notify_one(); // empurra os movimentos (ledger) no sync
        });
    });
}

/// Callback: cria um novo produto e atualiza a lista.
///
/// Regras aplicadas (AI_RULES.md §7.3, §7.4, §8):
/// - Após escrita bem-sucedida, dispara sync imediata via Notify
/// - Leitura e limpeza do form extraídas em helpers (§8 — max 30-50 linhas)
pub(crate) fn setup_add(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
    cache: Arc<std::sync::Mutex<Vec<DecodedProduct>>>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.global::<ProductsState>().on_add_product(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };

        if !validate_product_form(&ui_ref) {
            return;
        }

        let form = read_product_form(&ui_ref);
        let gallery = read_gallery_images(&ui_ref);
        let cat_name = ui_ref.global::<ProductsState>().get_product_category_name().to_string();
        let sub_name = ui_ref.global::<ProductsState>().get_product_subcategory_name().to_string();

        let ui_weak = ui_ref.as_weak();
        let state = state.clone();
        let notify = sync_notify.clone();
        let cache = cache.clone();

        handle.spawn(async move {
            let result = state.product_service
                .create(
                    state.company_id(), form.name, form.description,
                    form.category_id, form.subcategory_id,
                    form.price.map(letaf_core::money::from_db_f64), form.cost_price.map(letaf_core::money::from_db_f64), form.stock_quantity, form.min_stock, form.unlimited_stock,
                    form.barcode, form.unit, form.balance_mode, form.image_data,
                    form.cover_color, form.availability_schedule,
                    form.discount_kind, form.discount_value.map(letaf_core::money::from_db_f64), form.discount_min_qty,
                    form.discount_tiers,
                    form.addon_group_ids,
                    form.ingredients,
                    form.variations,
                )
                .await;

            // Galeria (loja): persiste as imagens adicionais após salvar o
            // produto (o create/update já marcou para sincronizar).
            if let Ok(p) = result.as_ref() {
                if let Err(e) = state.product_service.replace_images(state.company_id(), p.base.id, gallery).await {
                    tracing::error!("Falha ao salvar galeria do produto {}: {e}", p.base.id);
                }
            }

            if result.is_ok() { notify.notify_one(); }

            // Miniatura da lista: derivada da imagem recém-salva, gravada
            // sem tocar em `updated_at` (não é alteração de dado, e marcá-la
            // faria o produto reempurrar no sync a cada terminal).
            if let Ok(p) = result.as_ref() {
                gerar_miniatura(&state, p).await;
            }

            match result {
                Ok(p) => {
                    let pixel_buf = decode_single_product_image(p.image_data.clone()).await;
                    let p_name = p.name.clone();
                    let new_id = SharedString::from(p.base.id.to_string());
                    // Atualiza o cache ANTES de tocar a UI — `select-product`
                    // lê daqui para preencher o form quando o operador
                    // volta no produto.
                    let decoded = decoded_from_components(&p, &cat_name, &sub_name, pixel_buf.clone());
                    upsert_decoded_in_cache(&cache, decoded);
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        let p_data = build_product_data_from_product(&p, &cat_name, &sub_name, pixel_buf);
                        push_product_to_model(&ui, p_data.clone());
                        ui.set_editing_id(new_id.clone());
                        ui.global::<ProductsState>().set_selected_product_id(new_id);
                        ui.global::<ProductsState>().set_detail_product(p_data);
                        ui.global::<ProductsState>().set_product_save_error(SharedString::default());
                        show_toast(&ui, &format!("Produto '{}' Criado", p_name), "success");
                        ui.set_status_message(SharedString::from(format!("Produto '{}' Criado", p_name)));
                    });
                }
                Err(e) => {
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        let msg = SharedString::from(friendly_error(&e));
                        show_toast(&ui, msg.as_str(), "error");
                        ui.set_status_message(msg.clone());
                        ui.global::<ProductsState>().set_product_save_error(msg);
                    });
                }
            }
        });
    });
}

/// Callback: atualiza um produto existente.
pub(crate) fn setup_update_product(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
    cache: Arc<std::sync::Mutex<Vec<DecodedProduct>>>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.global::<ProductsState>().on_update_product(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };

        if !validate_product_form(&ui_ref) {
            return;
        }

        let id_str = ui_ref.get_editing_id().to_string();
        let Ok(id) = Uuid::parse_str(&id_str) else { return };
        let form = read_product_form(&ui_ref);
        let gallery = read_gallery_images(&ui_ref);
        let cat_name = ui_ref.global::<ProductsState>().get_product_category_name().to_string();
        let sub_name = ui_ref.global::<ProductsState>().get_product_subcategory_name().to_string();
        let id_ss = SharedString::from(id_str.as_str());

        let ui_weak = ui_ref.as_weak();
        let state = state.clone();
        let notify = sync_notify.clone();
        let cache = cache.clone();

        handle.spawn(async move {
            let cid = state.company_id();
            let result = state.product_service
                .update(
                    cid, id, form.name, form.description,
                    form.category_id, form.subcategory_id,
                    form.price.map(letaf_core::money::from_db_f64), form.cost_price.map(letaf_core::money::from_db_f64), form.stock_quantity, form.min_stock, form.unlimited_stock,
                    form.barcode, form.unit, form.balance_mode, form.image_data,
                    form.cover_color, form.availability_schedule,
                    form.discount_kind, form.discount_value.map(letaf_core::money::from_db_f64), form.discount_min_qty,
                    form.discount_tiers,
                    form.addon_group_ids,
                    form.ingredients,
                    form.variations,
                )
                .await;

            // Galeria (loja): persiste as imagens adicionais após salvar o
            // produto (o create/update já marcou para sincronizar).
            if let Ok(p) = result.as_ref() {
                if let Err(e) = state.product_service.replace_images(state.company_id(), p.base.id, gallery).await {
                    tracing::error!("Falha ao salvar galeria do produto {}: {e}", p.base.id);
                }
            }

            if result.is_ok() { notify.notify_one(); }

            // Miniatura da lista: derivada da imagem recém-salva, gravada
            // sem tocar em `updated_at` (não é alteração de dado, e marcá-la
            // faria o produto reempurrar no sync a cada terminal).
            if let Ok(p) = result.as_ref() {
                gerar_miniatura(&state, p).await;
            }

            match result {
                Ok(p) => {
                    let pixel_buf = decode_single_product_image(p.image_data.clone()).await;
                    let p_name = p.name.clone();
                    // Reescreve a entrada no cache ANTES do event loop —
                    // sem isso, `setup_select_product` (que lê do cache)
                    // mostraria a versão antiga ao voltar no produto.
                    let decoded = decoded_from_components(&p, &cat_name, &sub_name, pixel_buf.clone());
                    upsert_decoded_in_cache(&cache, decoded);
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        let p_data = build_product_data_from_product(&p, &cat_name, &sub_name, pixel_buf);
                        replace_product_in_model(&ui, &id_ss, p_data.clone());
                        ui.global::<ProductsState>().set_detail_product(p_data);
                        ui.global::<ProductsState>().set_product_margin_display(
                            ui.global::<ProductsState>().get_detail_product().margin_pct_display
                        );
                        ui.global::<ProductsState>().set_product_save_error(SharedString::default());
                        show_toast(&ui, &format!("Produto '{}' Atualizado", p_name), "success");
                        ui.set_status_message(SharedString::from(format!("Produto '{}' Atualizado", p_name)));
                    });
                }
                Err(e) => {
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        let msg = SharedString::from(friendly_error(&e));
                        show_toast(&ui, msg.as_str(), "error");
                        ui.set_status_message(msg.clone());
                        ui.global::<ProductsState>().set_product_save_error(msg);
                    });
                }
            }
        });
    });
}

/// Callback: remove logicamente um produto e atualiza a lista.
///
/// Regras aplicadas (AI_RULES.md §7.3, §7.4):
/// - Após escrita bem-sucedida, dispara sync imediata via Notify
pub(crate) fn setup_delete(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.global::<ProductsState>().on_delete_product(move |id_str| {
        let id = match Uuid::parse_str(id_str.as_str()) {
            Ok(id) => id,
            Err(e) => {
                tracing::error!("Invalid product ID: {e}");
                return;
            }
        };

        let ui_weak = ui_weak.clone();
        let state = state.clone();

        let notify = sync_notify.clone();

        handle.spawn(async move {
            let result = state.product_service
                .soft_delete(state.company_id(), id).await;

            if result.is_ok() { notify.notify_one(); }

            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                match result {
                    Ok(()) => {
                        show_toast(&ui, "Produto exclu\u{ed}do", "success");
                        ui.set_status_message("Produto exclu\u{ed}do".into());
                        ui.global::<ProductsState>().invoke_refresh_products();
                    }
                    Err(e) => {
                        let msg = friendly_error(&e);
                        show_toast(&ui, &msg, "error");
                        ui.set_status_message(SharedString::from(msg));
                    }
                }
            });
        });
    });
}

/// Gera e grava a miniatura do produto a partir da imagem salva.
///
/// Falha só loga: a miniatura é otimização de leitura da lista, não dado do
/// negócio — não pode derrubar o salvamento do produto.
async fn gerar_miniatura(state: &DesktopState, p: &letaf_core::product::model::Product) {
    let Some(img) = p.image_data.clone() else { return };
    if img.is_empty() {
        return;
    }
    // Decodificar e reescalar é CPU-bound: fora do executor do Tokio.
    let thumb = tokio::task::spawn_blocking(move || {
        crate::ui::image::make_thumbnail(&img)
    })
    .await
    .ok()
    .flatten();
    if let Err(e) = state
        .product_service
        .set_thumbnail(p.base.company_id, p.base.id, thumb.as_deref())
        .await
    {
        tracing::warn!("miniatura do produto {}: {e}", p.base.id);
    }
}
