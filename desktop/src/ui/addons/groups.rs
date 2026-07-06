use std::sync::Arc;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use tokio::sync::Notify;
use uuid::Uuid;

use letaf_core::addon::model::Addon;
use letaf_core::addon_group::model::AddonGroup;

use crate::context::DesktopState;
use crate::{AddonData, AddonGroupData, MainWindow};

use super::super::helpers::show_toast;

/// Refresh: carrega grupos + (em paralelo) todos os addons para já saber
/// quantos itens cada grupo tem (`addons-count`). Mantemos a contagem
/// na UI pra evitar refetch a cada clique.
pub(crate) fn setup_refresh_addon_groups(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_refresh_addon_groups(move || {
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            let (groups_res, addons_res) = tokio::join!(
                state.addon_group_service.find_all(cid),
                state.addon_service.find_all(cid),
            );
            let groups = match groups_res {
                Ok(g) => g,
                Err(e) => return ui_status_err(&ui_weak, e),
            };
            let addons = addons_res.unwrap_or_default();
            let group_data = build_groups_with_counts(&groups, &addons, None);
            // Também monta a lista usada como chips no form do produto:
            // por enquanto sem nenhum selecionado — o `request-edit` do
            // produto reescreve com flag `selected` por linha.
            let product_chips = build_groups_with_counts(&groups, &addons, Some(&[]));

            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                ui.set_addon_groups(ModelRc::new(VecModel::from(group_data)));
                ui.set_product_addon_groups(ModelRc::new(VecModel::from(product_chips)));
            });
        });
    });
}

/// Constrói `Vec<AddonGroupData>` aplicando contagem e (opcional) flag
/// `selected` quando estiver montando a lista usada no form do produto.
pub(crate) fn build_groups_with_counts(
    groups: &[AddonGroup],
    addons: &[Addon],
    selected_ids: Option<&[Uuid]>,
) -> Vec<AddonGroupData> {
    groups.iter()
        .map(|g| {
            let count = addons.iter().filter(|a| a.group_id == g.base.id).count() as i32;
            let selected = selected_ids
                .map(|ids| ids.contains(&g.base.id))
                .unwrap_or(false);
            AddonGroupData {
                id: SharedString::from(g.base.id.to_string()),
                name: SharedString::from(g.name.as_str()),
                selection: SharedString::from(g.selection.as_str()),
                min_select: g.min_select,
                max_select: g.max_select,
                sort_order: g.sort_order,
                addons_count: count,
                selected,
            }
        })
        .collect()
}

/// Atalho: imprime erro do service na status bar do MainWindow.
pub(crate) fn ui_status_err(ui_weak: &slint::Weak<MainWindow>, e: impl std::fmt::Display) {
    let msg = format!("Erro: {e}");
    let ui_weak = ui_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_status_message(SharedString::from(msg));
        }
    });
}

/// Carrega os addons de um grupo (coluna direita) e atualiza UI.
pub(crate) fn setup_select_addon_group(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_select_addon_group(move |id| {
        let id_str = id.to_string();
        let Ok(gid) = Uuid::parse_str(&id_str) else { return };
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            let name = state.addon_group_service.find_by_id(cid, gid).await
                .ok().flatten()
                .map(|g| g.name)
                .unwrap_or_default();
            let addons = state.addon_service.find_by_group(cid, gid).await.unwrap_or_default();
            let data: Vec<AddonData> = addons.iter().map(addon_to_ui).collect();
            let id_ss = SharedString::from(id_str.as_str());
            let name_ss = SharedString::from(name);
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                ui.set_selected_addon_group_id(id_ss);
                ui.set_selected_addon_group_name(name_ss);
                ui.set_current_group_addons(ModelRc::new(VecModel::from(data)));
            });
        });
    });
}

pub(crate) fn addon_to_ui(a: &Addon) -> AddonData {
    AddonData {
        id: SharedString::from(a.base.id.to_string()),
        group_id: SharedString::from(a.group_id.to_string()),
        name: SharedString::from(a.name.as_str()),
        price: SharedString::from(format_price(a.price)),
        sort_order: a.sort_order,
        active: a.active,
    }
}

pub(crate) fn format_price(p: f64) -> String {
    if p.fract() == 0.0 { format!("{:.0}", p) } else { format!("{p:.2}") }
}

pub(crate) fn parse_decimal(raw: &SharedString) -> Option<f64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() { return None; }
    trimmed.replace(',', ".").parse::<f64>().ok()
}

pub(crate) fn parse_int(raw: &SharedString) -> Option<i32> {
    let trimmed = raw.trim();
    if trimmed.is_empty() { return Some(0); }
    trimmed.parse::<i32>().ok()
}

/// Save group: cria (id vazio) ou atualiza. UI valida apenas presença
/// de nome; resto cai no service.
pub(crate) fn setup_save_addon_group(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_save_addon_group(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        let id_str = ui_ref.get_addon_group_form_id().to_string();
        let name = ui_ref.get_addon_group_form_name().trim().to_string();
        if name.is_empty() {
            ui_ref.set_addon_group_form_error(SharedString::from("Preencha o nome do grupo"));
            return;
        }
        let selection = ui_ref.get_addon_group_form_selection().to_string();
        let min = parse_int(&ui_ref.get_addon_group_form_min()).unwrap_or(0);
        let max = parse_int(&ui_ref.get_addon_group_form_max()).unwrap_or(0);
        ui_ref.set_addon_group_form_error(SharedString::default());

        let ui_weak = ui_ref.as_weak();
        let state = state.clone();
        let notify = sync_notify.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            let res = if id_str.is_empty() {
                state.addon_group_service
                    .create(cid, name.clone(), selection.clone(), min, max).await
                    .map(|_| ())
            } else {
                match Uuid::parse_str(&id_str) {
                    Ok(id) => state.addon_group_service
                        .update(cid, id, name.clone(), selection.clone(), min, max).await
                        .map(|_| ()),
                    Err(_) => Err(letaf_core::error::CoreError::Validation("Invalid id".into())),
                }
            };
            if res.is_ok() { notify.notify_one(); }
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                match res {
                    Ok(()) => {
                        show_toast(&ui, "Grupo salvo", "success");
                        ui.set_show_modal(false);
                        ui.invoke_refresh_addon_groups();
                    }
                    Err(e) => {
                        let msg = format!("Erro: {e}");
                        show_toast(&ui, &msg, "error");
                        ui.set_addon_group_form_error(SharedString::from(msg));
                    }
                }
            });
        });
    });
}

/// Deletar grupo (chamado pelo confirm-delete unificado em mod.rs).
pub(crate) fn setup_delete_addon_group(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_delete_addon_group(move |id_str| {
        let Ok(id) = Uuid::parse_str(id_str.as_str()) else { return };
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let notify = sync_notify.clone();
        handle.spawn(async move {
            let res = state.addon_group_service.soft_delete(state.company_id(), id).await;
            if res.is_ok() { notify.notify_one(); }
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                match res {
                    Ok(()) => {
                        show_toast(&ui, "Grupo excluído", "success");
                        // Se era o grupo focado, limpa a coluna direita.
                        if ui.get_selected_addon_group_id() == id.to_string() {
                            ui.set_selected_addon_group_id(SharedString::default());
                            ui.set_selected_addon_group_name(SharedString::default());
                            ui.set_current_group_addons(ModelRc::new(VecModel::<AddonData>::from(Vec::new())));
                        }
                        ui.invoke_refresh_addon_groups();
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

