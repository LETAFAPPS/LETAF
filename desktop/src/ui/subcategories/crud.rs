use std::collections::HashMap;
use std::sync::Arc;

use slint::{ComponentHandle, SharedString};
use tokio::sync::Notify;
use uuid::Uuid;

use letaf_core::subcategory::model::Subcategory;

use crate::context::DesktopState;
use crate::{MainWindow, SubcategoryData};

use super::super::helpers::show_toast;
use super::list::{clear_subcategory_form, validate_subcategory_form};

/// Callback: cria subcategoria e dispara sync.
pub(crate) fn setup_add_subcategory(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_add_subcategory(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };

        if !validate_subcategory_form(&ui_ref) {
            return;
        }

        let name = ui_ref.get_subcategory_name().to_string();
        let cat_id_str = ui_ref.get_subcategory_category_id().to_string();
        let Ok(category_id) = Uuid::parse_str(&cat_id_str) else {
            ui_ref.set_subcategory_error_category(SharedString::from("Categoria inválida"));
            return;
        };

        let ui_weak = ui_ref.as_weak();
        let state = state.clone();
        let notify = sync_notify.clone();

        handle.spawn(async move {
            let cid = state.company_id();
            match state.subcategory_service.create(cid, category_id, name).await {
                Ok(_) => {
                    notify.notify_one();
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        show_toast(&ui, "Subcategoria Criada", "success");
                        clear_subcategory_form(&ui);
                        ui.set_show_modal(false);
                        ui.set_status_message(SharedString::from("Subcategoria Criada"));
                        ui.invoke_refresh_subcategories();
                        ui.invoke_refresh_categories();
                    });
                }
                Err(e) => {
                    let msg = format!("Erro: {e}");
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        show_toast(&ui, &msg, "error");
                        ui.set_status_message(SharedString::from(msg));
                    });
                }
            }
        });
    });
}

/// Callback: atualiza uma subcategoria existente.
pub(crate) fn setup_update_subcategory(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_update_subcategory(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };

        if !validate_subcategory_form(&ui_ref) {
            return;
        }

        let id_str = ui_ref.get_editing_id().to_string();
        let Ok(id) = Uuid::parse_str(&id_str) else { return };
        let name = ui_ref.get_subcategory_name().to_string();
        let cat_id_str = ui_ref.get_subcategory_category_id().to_string();
        let Ok(category_id) = Uuid::parse_str(&cat_id_str) else {
            ui_ref.set_subcategory_error_category(SharedString::from("Categoria inválida"));
            return;
        };

        let ui_weak = ui_ref.as_weak();
        let state = state.clone();
        let notify = sync_notify.clone();

        handle.spawn(async move {
            let cid = state.company_id();
            match state.subcategory_service.update(cid, id, category_id, name).await {
                Ok(s) => {
                    notify.notify_one();
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        show_toast(&ui, &format!("Subcategoria '{}' Atualizada", s.name), "success");
                        clear_subcategory_form(&ui);
                        ui.set_editing_id(SharedString::default());
                        ui.set_show_modal(false);
                        ui.set_status_message(SharedString::from(format!("Subcategoria '{}' Atualizada", s.name)));
                        ui.invoke_refresh_subcategories();
                        ui.invoke_refresh_categories();
                    });
                }
                Err(e) => {
                    let msg = format!("Erro: {e}");
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        show_toast(&ui, &msg, "error");
                        ui.set_status_message(SharedString::from(msg));
                    });
                }
            }
        });
    });
}

/// Callback: soft-delete de subcategoria e dispara sync.
pub(crate) fn setup_delete_subcategory(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_delete_subcategory(move |id_str| {
        let Ok(id) = Uuid::parse_str(id_str.as_str()) else { return };

        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let notify = sync_notify.clone();

        handle.spawn(async move {
            let cid = state.company_id();
            match state.subcategory_service.soft_delete(cid, id).await {
                Ok(_) => {
                    notify.notify_one();
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        show_toast(&ui, "Subcategoria Excluída", "success");
                        ui.set_status_message(SharedString::from("Subcategoria Excluída"));
                        ui.invoke_refresh_subcategories();
                        ui.invoke_refresh_categories();
                    });
                }
                Err(e) => {
                    let msg = format!("Erro: {e}");
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        show_toast(&ui, &msg, "error");
                        ui.set_status_message(SharedString::from(msg));
                    });
                }
            }
        });
    });
}

/// Callback: reordena subcategorias (move para cima ou para baixo).
pub(crate) fn setup_reorder_subcategory(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_reorder_subcategory(move |id_str, is_up| {
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let notify = sync_notify.clone();

        let Ok(target_id) = Uuid::parse_str(id_str.as_str()) else { return };

        handle.spawn(async move {
            let cid = state.company_id();
            let Ok(items) = state.subcategory_service.find_all(cid).await else { return };

            let Some(idx) = items.iter().position(|s| s.base.id == target_id) else { return };

            let swap_idx = if is_up {
                if idx == 0 { return; }
                idx - 1
            } else {
                if idx + 1 >= items.len() { return; }
                idx + 1
            };

            let mut ordered: Vec<Uuid> = items.iter().map(|s| s.base.id).collect();
            ordered.swap(idx, swap_idx);

            for (new_order, &id) in ordered.iter().enumerate() {
                let old_order = items.iter().find(|s| s.base.id == id).map(|s| s.sort_order).unwrap_or(0);
                if old_order != new_order as i32 {
                    if let Err(e) = state.subcategory_service.update_sort_order(cid, id, new_order as i32).await {
                        tracing::error!("Erro ao reordenar subcategoria: {e}");
                        return;
                    }
                }
            }

            notify.notify_one();
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                ui.invoke_refresh_categories();
                ui.invoke_refresh_subcategories();
            });
        });
    });
}

/// Converte Subcategory do domínio para SubcategoryData do Slint.
///
/// `cat_names` é o mapa pré-carregado de `category_id -> name` para
/// evitar uma query por linha (join feito em memória — §8 e §10).
pub(crate) fn to_subcategory_data(s: &Subcategory, cat_names: &HashMap<Uuid, String>) -> SubcategoryData {
    let category_name = cat_names
        .get(&s.category_id)
        .cloned()
        .unwrap_or_else(|| "(categoria removida)".into());
    SubcategoryData {
        id: SharedString::from(s.base.id.to_string()),
        name: SharedString::from(s.name.as_str()),
        category_id: SharedString::from(s.category_id.to_string()),
        category_name: SharedString::from(category_name),
        sort_order: s.sort_order,
    }
}
