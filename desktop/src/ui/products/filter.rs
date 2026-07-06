use std::collections::HashSet;
use std::sync::Arc;

use slint::{ComponentHandle, SharedString};


use crate::MainWindow;

use super::state::{DecodedProduct, SharedFilter};
use super::list::refresh_products_view;

/// Setup do callback de busca textual (mantém o estado de query e re-aplica).
pub(crate) fn setup_filter_products(
    ui: &MainWindow,
    cache: Arc<std::sync::Mutex<Vec<DecodedProduct>>>,
    filter: SharedFilter,
) {
    let ui_weak = ui.as_weak();
    ui.on_filter_products(move |query| {
        let Some(ui) = ui_weak.upgrade() else { return };
        if let Ok(mut f) = filter.lock() {
            f.search_query = query.to_string();
        }
        refresh_products_view(&ui, &cache, &filter);
    });
}

/// Toggle de seleção de categoria (no HashSet) + re-aplica filtros.
pub(crate) fn setup_toggle_category_filter(
    ui: &MainWindow,
    cache: Arc<std::sync::Mutex<Vec<DecodedProduct>>>,
    filter: SharedFilter,
) {
    let ui_weak = ui.as_weak();
    ui.on_toggle_category_filter(move |id| {
        let Some(ui) = ui_weak.upgrade() else { return };
        if let Ok(mut f) = filter.lock() {
            let id_s = id.to_string();
            if !f.selected_categories.remove(&id_s) {
                f.selected_categories.insert(id_s);
            }
            // Limpa subcategorias cuja categoria-pai saiu da seleção.
            let valid_cats: HashSet<String> = f.selected_categories.clone();
            let valid_subs: HashSet<String> = f.known_subcategories.iter()
                .filter(|(_, cat_id, _)| {
                    valid_cats.is_empty() || valid_cats.contains(cat_id)
                })
                .map(|(id, _, _)| id.clone())
                .collect();
            f.selected_subcategories.retain(|id| valid_subs.contains(id));
        }
        refresh_products_view(&ui, &cache, &filter);
    });
}

pub(crate) fn setup_toggle_subcategory_filter(
    ui: &MainWindow,
    cache: Arc<std::sync::Mutex<Vec<DecodedProduct>>>,
    filter: SharedFilter,
) {
    let ui_weak = ui.as_weak();
    ui.on_toggle_subcategory_filter(move |id| {
        let Some(ui) = ui_weak.upgrade() else { return };
        if let Ok(mut f) = filter.lock() {
            let id_s = id.to_string();
            if !f.selected_subcategories.remove(&id_s) {
                f.selected_subcategories.insert(id_s);
            }
        }
        refresh_products_view(&ui, &cache, &filter);
    });
}

pub(crate) fn setup_set_status_filter(
    ui: &MainWindow,
    cache: Arc<std::sync::Mutex<Vec<DecodedProduct>>>,
    filter: SharedFilter,
) {
    let ui_weak = ui.as_weak();
    ui.on_set_status_filter(move |value| {
        let Some(ui) = ui_weak.upgrade() else { return };
        if let Ok(mut f) = filter.lock() {
            f.status = value.to_string();
        }
        ui.set_filter_status(value);
        refresh_products_view(&ui, &cache, &filter);
    });
}

pub(crate) fn setup_set_stock_filter(
    ui: &MainWindow,
    cache: Arc<std::sync::Mutex<Vec<DecodedProduct>>>,
    filter: SharedFilter,
) {
    let ui_weak = ui.as_weak();
    ui.on_set_stock_filter(move |value| {
        let Some(ui) = ui_weak.upgrade() else { return };
        if let Ok(mut f) = filter.lock() {
            f.stock = value.to_string();
        }
        ui.set_filter_stock(value);
        refresh_products_view(&ui, &cache, &filter);
    });
}

pub(crate) fn setup_reset_product_filters(
    ui: &MainWindow,
    cache: Arc<std::sync::Mutex<Vec<DecodedProduct>>>,
    filter: SharedFilter,
) {
    let ui_weak = ui.as_weak();
    ui.on_reset_product_filters(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        if let Ok(mut f) = filter.lock() {
            f.selected_categories.clear();
            f.selected_subcategories.clear();
            f.status = "both".to_string();
            f.stock  = "both".to_string();
        }
        ui.set_filter_status(SharedString::from("both"));
        ui.set_filter_stock(SharedString::from("both"));
        refresh_products_view(&ui, &cache, &filter);
    });
}

