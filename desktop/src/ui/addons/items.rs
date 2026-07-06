use std::sync::Arc;

use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use tokio::sync::Notify;
use uuid::Uuid;


use crate::context::DesktopState;
use crate::MainWindow;

use super::super::helpers::{show_toast, user_error};
use super::groups::{build_groups_with_counts, parse_decimal};

/// Save addon: usa o `selected-addon-group-id` como contexto.
pub(crate) fn setup_save_addon(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_save_addon(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        let group_id_str = ui_ref.get_selected_addon_group_id().to_string();
        let Ok(group_id) = Uuid::parse_str(&group_id_str) else { return };
        let id_str = ui_ref.get_addon_form_id().to_string();
        let name = ui_ref.get_addon_form_name().trim().to_string();
        if name.is_empty() {
            ui_ref.set_addon_form_error(SharedString::from("Preencha o nome do adicional"));
            return;
        }
        let price = parse_decimal(&ui_ref.get_addon_form_price()).unwrap_or(0.0);
        ui_ref.set_addon_form_error(SharedString::default());

        let ui_weak = ui_ref.as_weak();
        let state = state.clone();
        let notify = sync_notify.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            let res = if id_str.is_empty() {
                state.addon_service.create(cid, group_id, name.clone(), price).await.map(|_| ())
            } else {
                match Uuid::parse_str(&id_str) {
                    Ok(id) => state.addon_service
                        .update(cid, id, group_id, name.clone(), price).await
                        .map(|_| ()),
                    Err(_) => Err(letaf_core::error::CoreError::Validation("Invalid id".into())),
                }
            };
            if res.is_ok() { notify.notify_one(); }
            let group_id_ss = SharedString::from(group_id.to_string());
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                match res {
                    Ok(()) => {
                        show_toast(&ui, "Adicional salvo", "success");
                        ui.set_show_modal(false);
                        // Recarrega a coluna direita e a contagem do
                        // grupo na coluna esquerda.
                        ui.invoke_refresh_addon_groups();
                        ui.invoke_select_addon_group(group_id_ss);
                    }
                    Err(e) => {
                        let msg = format!("Erro: {e}");
                        show_toast(&ui, &msg, "error");
                        ui.set_addon_form_error(SharedString::from(msg));
                    }
                }
            });
        });
    });
}

pub(crate) fn setup_delete_addon(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_delete_addon(move |id_str| {
        let Ok(id) = Uuid::parse_str(id_str.as_str()) else { return };
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let notify = sync_notify.clone();
        handle.spawn(async move {
            let res = state.addon_service.soft_delete(state.company_id(), id).await;
            if res.is_ok() { notify.notify_one(); }
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                match res {
                    Ok(()) => {
                        show_toast(&ui, "Adicional excluído", "success");
                        let gid = ui.get_selected_addon_group_id();
                        ui.invoke_refresh_addon_groups();
                        if !gid.is_empty() { ui.invoke_select_addon_group(gid); }
                    }
                    Err(e) => {
                        let msg = format!("Erro: {e}");
                        show_toast(&ui, &msg, "error");
                    }
                }
            });
        });
    });
}

/// Alterna `active` do addon (botão "Ativo/Inativo" da linha).
pub(crate) fn setup_toggle_addon_active(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_toggle_addon_active(move |id_str| {
        let Ok(id) = Uuid::parse_str(id_str.as_str()) else { return };
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let notify = sync_notify.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            let current = state.addon_service.find_by_id(cid, id).await
                .ok().flatten()
                .map(|a| a.active)
                .unwrap_or(false);
            let res = state.addon_service.toggle_active(cid, id, !current).await;
            if res.is_ok() { notify.notify_one(); }
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                let gid = ui.get_selected_addon_group_id();
                if !gid.is_empty() { ui.invoke_select_addon_group(gid); }
                if let Err(e) = res {
                    show_toast(&ui, &user_error(&e), "error");
                }
            });
        });
    });
}

/// Toggle do chip de grupo no form de produto. Encontra a linha pelo
/// id e inverte `selected` in-place. Persistência ocorre só no save do
/// produto (`product_service.update`).
pub(crate) fn setup_toggle_product_addon_group(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_toggle_product_addon_group(move |id| {
        let Some(ui) = ui_weak.upgrade() else { return };
        let model = ui.get_product_addon_groups();
        for i in 0..model.row_count() {
            let Some(mut row) = model.row_data(i) else { continue };
            if row.id == id {
                row.selected = !row.selected;
                model.set_row_data(i, row);
            }
        }
    });
}

/// Popula a lista `product-addon-groups` (chips) com os grupos atuais
/// e marca como `selected` os IDs informados (usado no `request-edit`
/// do produto). Chamado a partir do `products.rs`.
pub(crate) fn refresh_product_addon_groups(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    selected_ids: Vec<Uuid>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    handle.spawn(async move {
        let cid = state.company_id();
        let (groups_res, addons_res) = tokio::join!(
            state.addon_group_service.find_all(cid),
            state.addon_service.find_all(cid),
        );
        let groups = groups_res.unwrap_or_default();
        let addons = addons_res.unwrap_or_default();
        let data = build_groups_with_counts(&groups, &addons, Some(&selected_ids));
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_product_addon_groups(ModelRc::new(VecModel::from(data)));
            }
        });
    });
}

/// Lê do VecModel apenas os grupos com `selected = true` — usado no
/// `read_product_form` ao salvar.
pub(crate) fn read_selected_addon_group_ids(ui: &MainWindow) -> Vec<Uuid> {
    let model = ui.get_product_addon_groups();
    let mut ids = Vec::new();
    for i in 0..model.row_count() {
        let Some(row) = model.row_data(i) else { continue };
        if row.selected {
            if let Ok(id) = Uuid::parse_str(row.id.as_str()) { ids.push(id); }
        }
    }
    ids
}
