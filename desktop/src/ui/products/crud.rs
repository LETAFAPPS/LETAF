use std::sync::Arc;

use slint::{ComponentHandle, SharedString};
use tokio::sync::Notify;
use uuid::Uuid;


use crate::context::DesktopState;
use crate::MainWindow;

use super::super::helpers::show_toast;
use super::super::image::decode_single_product_image;
use super::state::DecodedProduct;
use super::list::{decoded_from_components, upsert_decoded_in_cache};
use super::form::{read_product_form, validate_product_form};
use super::data::{build_product_data_from_product, push_product_to_model, replace_product_in_model};

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

    ui.on_add_product(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };

        if !validate_product_form(&ui_ref) {
            return;
        }

        let form = read_product_form(&ui_ref);
        let cat_name = ui_ref.get_product_category_name().to_string();
        let sub_name = ui_ref.get_product_subcategory_name().to_string();

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
                    form.variations,
                )
                .await;

            if result.is_ok() { notify.notify_one(); }

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
                        ui.set_selected_product_id(new_id);
                        ui.set_detail_product(p_data);
                        ui.set_product_save_error(SharedString::default());
                        show_toast(&ui, &format!("Produto '{}' Criado", p_name), "success");
                        ui.set_status_message(SharedString::from(format!("Produto '{}' Criado", p_name)));
                    });
                }
                Err(e) => {
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        let msg = SharedString::from(format!("Erro: {e}"));
                        show_toast(&ui, msg.as_str(), "error");
                        ui.set_status_message(msg.clone());
                        ui.set_product_save_error(msg);
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

    ui.on_update_product(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };

        if !validate_product_form(&ui_ref) {
            return;
        }

        let id_str = ui_ref.get_editing_id().to_string();
        let Ok(id) = Uuid::parse_str(&id_str) else { return };
        let form = read_product_form(&ui_ref);
        let cat_name = ui_ref.get_product_category_name().to_string();
        let sub_name = ui_ref.get_product_subcategory_name().to_string();
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
                    form.variations,
                )
                .await;

            if result.is_ok() { notify.notify_one(); }

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
                        ui.set_detail_product(p_data);
                        ui.set_product_margin_display(
                            ui.get_detail_product().margin_pct_display
                        );
                        ui.set_product_save_error(SharedString::default());
                        show_toast(&ui, &format!("Produto '{}' Atualizado", p_name), "success");
                        ui.set_status_message(SharedString::from(format!("Produto '{}' Atualizado", p_name)));
                    });
                }
                Err(e) => {
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        let msg = SharedString::from(format!("Erro: {e}"));
                        show_toast(&ui, msg.as_str(), "error");
                        ui.set_status_message(msg.clone());
                        ui.set_product_save_error(msg);
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

    ui.on_delete_product(move |id_str| {
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
                        ui.invoke_refresh_products();
                    }
                    Err(e) => {
                        let msg = format!("Erro: {e}");
                        show_toast(&ui, &msg, "error");
                        ui.set_status_message(SharedString::from(msg));
                    }
                }
            });
        });
    });
}

