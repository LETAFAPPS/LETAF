use std::sync::Arc;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use tokio::sync::Notify;
use uuid::Uuid;

use letaf_core::category::icons;
use letaf_core::category::model::Category;

use crate::context::DesktopState;
use crate::{CategoryData, CategoryIconOption, MainWindow};

use super::super::helpers::show_toast;

/// Limpa erro de validação do formulário de categoria.
pub(crate) fn clear_category_errors(ui: &MainWindow) {
    ui.set_category_error_name(SharedString::default());
}

/// Limpa formulário de categoria.
pub(crate) fn clear_category_form(ui: &MainWindow) {
    ui.set_category_name(SharedString::default());
    ui.set_category_description(SharedString::default());
    ui.set_category_icon_name(SharedString::default());
    clear_category_errors(ui);
}

/// Popula a allowlist de ícones de categoria a partir do core.
pub(crate) fn load_category_icon_options(ui: &MainWindow) {
    let opts: Vec<CategoryIconOption> = icons::ICONS
        .iter()
        .map(|(slug, label)| CategoryIconOption {
            slug: SharedString::from(*slug),
            label: SharedString::from(*label),
        })
        .collect();
    ui.set_category_icon_options(ModelRc::new(VecModel::from(opts)));
}

/// Valida campos obrigatórios do formulário de categoria.
pub(crate) fn validate_category_form(ui: &MainWindow) -> bool {
    let mut valid = true;
    clear_category_errors(ui);

    if ui.get_category_name().trim().is_empty() {
        ui.set_category_error_name(SharedString::from("Preencha o nome da categoria"));
        valid = false;
    }

    valid
}

/// Callback: cria categoria e dispara sync.
pub(crate) fn setup_add_category(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_add_category(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };

        let ui_weak = ui_ref.as_weak();
        let state = state.clone();
        let notify = sync_notify.clone();

        if !validate_category_form(&ui_ref) {
            return;
        }

        let name = ui_ref.get_category_name().to_string();
        let desc_raw = ui_ref.get_category_description().to_string();
        let description = if desc_raw.is_empty() { None } else { Some(desc_raw) };
        let icon_raw = ui_ref.get_category_icon_name().to_string();
        let icon_name = if icon_raw.is_empty() { None } else { Some(icon_raw) };

        handle.spawn(async move {
            let cid = state.company_id();
            match state.category_service.create(cid, name, description, icon_name).await {
                Ok(_) => {
                    notify.notify_one();
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        show_toast(&ui, "Categoria Criada", "success");
                        clear_category_form(&ui);
                        ui.set_show_modal(false);
                        ui.set_status_message(SharedString::from("Categoria Criada"));
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

/// Callback: atualiza uma categoria existente.
pub(crate) fn setup_update_category(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_update_category(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };

        if !validate_category_form(&ui_ref) {
            return;
        }

        let id_str = ui_ref.get_editing_id().to_string();
        let Ok(id) = Uuid::parse_str(&id_str) else { return };
        let name = ui_ref.get_category_name().to_string();
        let desc_raw = ui_ref.get_category_description().to_string();
        let description = if desc_raw.is_empty() { None } else { Some(desc_raw) };
        let icon_raw = ui_ref.get_category_icon_name().to_string();
        let icon_name = if icon_raw.is_empty() { None } else { Some(icon_raw) };

        let ui_weak = ui_ref.as_weak();
        let state = state.clone();
        let notify = sync_notify.clone();

        handle.spawn(async move {
            let cid = state.company_id();
            match state.category_service.update(cid, id, name, description, icon_name).await {
                Ok(c) => {
                    notify.notify_one();
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        show_toast(&ui, &format!("Categoria '{}' Atualizada", c.name), "success");
                        clear_category_form(&ui);
                        ui.set_editing_id(SharedString::default());
                        ui.set_show_modal(false);
                        ui.set_status_message(SharedString::from(format!("Categoria '{}' Atualizada", c.name)));
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

/// Callback: soft-delete de categoria e dispara sync.
pub(crate) fn setup_delete_category(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_delete_category(move |id_str| {
        let Ok(id) = uuid::Uuid::parse_str(id_str.as_str()) else { return };

        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let notify = sync_notify.clone();

        handle.spawn(async move {
            let cid = state.company_id();
            match state.category_service.soft_delete(cid, id).await {
                Ok(_) => {
                    notify.notify_one();
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        show_toast(&ui, "Categoria Excluída", "success");
                        ui.set_status_message(SharedString::from("Categoria Excluída"));
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

/// Callback: reordena categorias (move para cima ou para baixo).
pub(crate) fn setup_reorder_category(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_reorder_category(move |id_str, is_up| {
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let notify = sync_notify.clone();

        let Ok(target_id) = Uuid::parse_str(id_str.as_str()) else { return };

        handle.spawn(async move {
            let cid = state.company_id();
            let Ok(items) = state.category_service.find_all(cid).await else { return };

            let Some(idx) = items.iter().position(|c| c.base.id == target_id) else { return };

            let swap_idx = if is_up {
                if idx == 0 { return; }
                idx - 1
            } else {
                if idx + 1 >= items.len() { return; }
                idx + 1
            };

            let mut ordered: Vec<Uuid> = items.iter().map(|c| c.base.id).collect();
            ordered.swap(idx, swap_idx);

            for (new_order, &id) in ordered.iter().enumerate() {
                let old_order = items.iter().find(|c| c.base.id == id).map(|c| c.sort_order).unwrap_or(0);
                if old_order != new_order as i32 {
                    if let Err(e) = state.category_service.update_sort_order(cid, id, new_order as i32).await {
                        tracing::error!("Erro ao reordenar categoria: {e}");
                        return;
                    }
                }
            }

            notify.notify_one();
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                ui.invoke_refresh_categories();
            });
        });
    });
}

/// Converte Category do dominio para CategoryData do Slint.
pub(crate) fn to_category_data(c: &Category) -> CategoryData {
    CategoryData {
        id: SharedString::from(c.base.id.to_string()),
        name: SharedString::from(c.name.as_str()),
        description: SharedString::from(c.description.as_deref().unwrap_or("")),
        sort_order: c.sort_order,
        icon_name: SharedString::from(c.icon_name.as_deref().unwrap_or("")),
    }
}
