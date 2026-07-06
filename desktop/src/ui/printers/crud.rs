use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use uuid::Uuid;

use crate::MainWindow;
use crate::{PrinterCategoryRow, PrinterData};
use crate::context::DesktopState;

use super::super::helpers::{show_toast, user_error};
use super::print::{setup_refresh_available_printers, setup_test_print, to_printer_data};

/// Ponto de entrada chamado em `setup_callbacks`.
pub(crate) fn setup_printers(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
) {
    setup_refresh_printers(ui, state, handle);
    setup_save_printer(ui, state, handle);
    setup_delete_printer(ui, state, handle);
    setup_set_default_printer(ui, state, handle);
    setup_test_print(ui, state, handle);
    setup_refresh_available_printers(ui, handle);
    setup_load_printer_categories(ui, state, handle);
}

/// Popula `printer-form-categories` com as categorias da empresa,
/// marcando como `selected = true` os IDs em `selected-ids`.
///
/// Chamado quando o modal abre — em "Adicionar" passa `[]` (nada
/// pré-selecionado); em "Editar" passa `printer.category_ids` para
/// pré-marcar as escolhas existentes.
pub(crate) fn setup_load_printer_categories(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_load_printer_categories(move |selected_ids| {
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        // Snapshot dos IDs marcados (vem como ModelRc<SharedString>);
        // movemos para HashSet<String> para lookup O(1) durante a
        // construção das linhas.
        let mut selected: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        for i in 0..selected_ids.row_count() {
            if let Some(s) = selected_ids.row_data(i) {
                selected.insert(s.to_string());
            }
        }
        handle.spawn(async move {
            let cid = state.company_id();
            let cats = match state.category_service.find_all(cid).await {
                Ok(c) => c,
                Err(e) => { tracing::warn!("listar categorias para impressora: {e}"); return; }
            };
            let rows: Vec<PrinterCategoryRow> = cats.into_iter().map(|c| {
                let id_str = c.base.id.to_string();
                let was_selected = selected.contains(&id_str);
                PrinterCategoryRow {
                    id: SharedString::from(id_str),
                    name: SharedString::from(c.name),
                    selected: was_selected,
                }
            }).collect();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    ui.set_printer_form_categories(ModelRc::new(VecModel::from(rows)));
                }
            });
        });
    });
}

/// Carrega a lista do banco e popula `ui.printers`. Disparado na
/// abertura de Configurações e após cada save/delete/set-default.
pub(crate) fn setup_refresh_printers(ui: &MainWindow, state: &DesktopState, handle: &tokio::runtime::Handle) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_refresh_printers(move || {
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            let list = match state.printer_service.find_all(cid).await {
                Ok(l) => l,
                Err(e) => { tracing::warn!("listar impressoras: {e}"); return; }
            };
            // `PrinterData` contém `ModelRc` (não-Send) — não podemos
            // construir antes do `invoke_from_event_loop`. Convertemos
            // dentro da closure que já roda no event loop Slint.
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    let rows: Vec<PrinterData> = list.into_iter().map(to_printer_data).collect();
                    ui.set_printers(ModelRc::new(VecModel::from(rows)));
                }
            });
        });
    });
}

/// "Adicionar" / "Salvar" do modal. Lê os campos do form, chama
/// `create` ou `update` conforme `editing-id`, e refaz a lista.
pub(crate) fn setup_save_printer(ui: &MainWindow, state: &DesktopState, handle: &tokio::runtime::Handle) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_printer_save(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        let editing_id = ui_ref.get_printer_editing_id().to_string();
        let name = ui_ref.get_printer_form_name().to_string();
        let kind = ui_ref.get_printer_form_kind().to_string();
        let system_name = ui_ref.get_printer_form_system_name().to_string();
        let is_default = ui_ref.get_printer_form_is_default();
        let paper_width = ui_ref.get_printer_form_paper_width();
        // Lê as categorias selecionadas — apenas as marcadas (`selected
        // == true`) entram. Lista vazia ⇒ "catch-all" (recebe tudo).
        let categories_model = ui_ref.get_printer_form_categories();
        let mut category_ids: Vec<Uuid> = Vec::new();
        for i in 0..categories_model.row_count() {
            if let Some(row) = categories_model.row_data(i) {
                if row.selected {
                    if let Ok(uuid) = Uuid::parse_str(row.id.as_str()) {
                        category_ids.push(uuid);
                    }
                }
            }
        }
        let state = state.clone();
        let ui_weak = ui_weak.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            let result = if editing_id.is_empty() {
                state.printer_service
                    .create(cid, name, kind, system_name, is_default, paper_width, category_ids)
                    .await
                    .map(|_| ())
            } else {
                match Uuid::parse_str(&editing_id) {
                    Ok(id) => state.printer_service
                        .update(cid, id, name, kind, system_name, is_default, paper_width, category_ids)
                        .await
                        .map(|_| ()),
                    Err(_) => Err(letaf_core::error::CoreError::Validation("Invalid printer id".into())),
                }
            };
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                match result {
                    Ok(()) => {
                        ui.set_printer_form_error(SharedString::default());
                        ui.set_show_printer_modal(false);
                        show_toast(&ui, "Impressora Salva", "success");
                        ui.invoke_refresh_printers();
                    }
                    Err(e) => ui.set_printer_form_error(SharedString::from(format!("{e}"))),
                }
            });
        });
    });
}

/// Exclusão (soft-delete) via service. Refaz a lista após sucesso.
pub(crate) fn setup_delete_printer(ui: &MainWindow, state: &DesktopState, handle: &tokio::runtime::Handle) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_printer_delete(move |id_str| {
        let Ok(id) = Uuid::parse_str(id_str.as_str()) else { return };
        let state = state.clone();
        let ui_weak = ui_weak.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            let result = state.printer_service.soft_delete(cid, id).await;
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                match result {
                    Ok(()) => {
                        show_toast(&ui, "Impressora Removida", "success");
                        ui.invoke_refresh_printers();
                    }
                    Err(e) => show_toast(&ui, &user_error(&e), "error"),
                }
            });
        });
    });
}

/// Toggle "marcar como padrão" no botão estrela da listagem.
pub(crate) fn setup_set_default_printer(ui: &MainWindow, state: &DesktopState, handle: &tokio::runtime::Handle) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_printer_set_default(move |id_str| {
        let Ok(id) = Uuid::parse_str(id_str.as_str()) else { return };
        let state = state.clone();
        let ui_weak = ui_weak.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            let result = state.printer_service.set_default(cid, id).await;
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                match result {
                    Ok(()) => {
                        show_toast(&ui, "Impressora marcada como padrão", "success");
                        ui.invoke_refresh_printers();
                    }
                    Err(e) => show_toast(&ui, &user_error(&e), "error"),
                }
            });
        });
    });
}

