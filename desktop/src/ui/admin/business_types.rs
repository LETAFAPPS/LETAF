//! Catálogo de tipos de empresa: formulário (novo/editar/busca) e persistência.

use std::sync::Arc;

use slint::{ComponentHandle, SharedString};
use tokio::sync::RwLock;

use crate::{AdminState, MainWindow, HTTP_CLIENT};

use super::super::helpers::show_toast;
use super::cache::BusinessTypesCache;
use super::filters::apply_business_type_filter;
use super::http::{report, report_modal};

/// Formulário de tipo de empresa: novo (limpa) e editar (preenche do cache).
pub(super) fn setup_business_type_form(ui: &MainWindow, cache: &BusinessTypesCache) {
    setup_business_type_new(ui);
    setup_business_type_edit(ui, cache);
    setup_business_type_search(ui, cache);
}

/// "+": abre o modal de cadastro limpo.
fn setup_business_type_new(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.global::<AdminState>().on_business_type_new(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        let s = ui.global::<AdminState>();
        s.set_business_type_id(SharedString::new());
        s.set_business_type_name(SharedString::new());
        s.set_business_type_description(SharedString::new());
        s.set_business_type_sort(SharedString::new());
        s.set_business_type_active(true);
        s.set_business_type_modal_open(true);
    });
}

/// Ícone de editar: abre o modal pré-preenchido com o tipo do cache.
fn setup_business_type_edit(ui: &MainWindow, cache: &BusinessTypesCache) {
    let ui_weak = ui.as_weak();
    let cache = cache.clone();
    ui.global::<AdminState>().on_business_type_edit(move |id| {
        let Some(ui) = ui_weak.upgrade() else { return };
        let Ok(g) = cache.lock() else { return };
        if let Some(b) = g.iter().find(|b| b.id == id.as_str()) {
            let s = ui.global::<AdminState>();
            s.set_business_type_id(b.id.clone().into());
            s.set_business_type_name(b.name.clone().into());
            s.set_business_type_description(b.description.clone().into());
            s.set_business_type_sort(b.sort_order.to_string().into());
            s.set_business_type_active(b.active);
            s.set_business_type_modal_open(true);
        }
    });
}

/// Busca por nome (reaplica sobre o cache).
fn setup_business_type_search(ui: &MainWindow, cache: &BusinessTypesCache) {
    let ui_weak = ui.as_weak();
    let cache = cache.clone();
    ui.global::<AdminState>().on_filter_business_types(move || {
        if let Some(ui) = ui_weak.upgrade() {
            apply_business_type_filter(&ui, &cache);
        }
    });
}

/// Salvar (criar/atualizar) e excluir tipo de empresa.
pub(super) fn setup_business_type_persist(
    ui: &MainWindow,
    handle: &tokio::runtime::Handle,
    auth_token: &Arc<RwLock<Option<String>>>,
    server_url: &str,
) {
    setup_business_type_save(ui, handle, auth_token, server_url);
    setup_business_type_delete(ui, handle, auth_token, server_url);
}

/// Campos do formulário de tipo de empresa (com o corpo JSON já montado).
struct BusinessTypeForm {
    id: String,
    name: String,
    body: serde_json::Value,
}

/// Salvar.
fn setup_business_type_save(
    ui: &MainWindow,
    handle: &tokio::runtime::Handle,
    auth_token: &Arc<RwLock<Option<String>>>,
    server_url: &str,
) {
    let ui_weak = ui.as_weak();
    let handle = handle.clone();
    let auth_token = auth_token.clone();
    let server_url = server_url.to_string();
    ui.global::<AdminState>().on_business_type_save(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        let form = read_business_type_form(&ui);
        if form.name.is_empty() {
            show_toast(&ui, "Informe o nome do tipo de empresa", "error");
            return;
        }
        let BusinessTypeForm { id, body, .. } = form;
        let ui_weak = ui.as_weak();
        let auth_token = auth_token.clone();
        let server_url = server_url.clone();
        handle.spawn(async move {
            let Some(token) = auth_token.read().await.clone() else { return };
            let result = if id.is_empty() {
                HTTP_CLIENT
                    .post(format!("{server_url}/admin/business-types"))
                    .bearer_auth(&token)
                    .json(&body)
                    .send()
                    .await
            } else {
                HTTP_CLIENT
                    .put(format!("{server_url}/admin/business-types/{id}"))
                    .bearer_auth(&token)
                    .json(&body)
                    .send()
                    .await
            };
            report_modal(ui_weak, result, "Tipo de empresa salvo", |ui| {
                ui.global::<AdminState>().set_business_type_modal_open(false);
            })
            .await;
        });
    });
}

/// Lê o formulário (já com o corpo JSON montado).
fn read_business_type_form(ui: &MainWindow) -> BusinessTypeForm {
    let g = ui.global::<AdminState>();
    let id = g.get_business_type_id().to_string();
    let name = g.get_business_type_name().trim().to_string();
    let description = g.get_business_type_description().trim().to_string();
    let sort_order: i32 = g.get_business_type_sort().trim().parse().unwrap_or(0);
    let active = g.get_business_type_active();
    let body = serde_json::json!({
        "name": name, "description": description,
        "sort_order": sort_order, "active": active,
    });
    BusinessTypeForm { id, name, body }
}

/// Excluir.
fn setup_business_type_delete(
    ui: &MainWindow,
    handle: &tokio::runtime::Handle,
    auth_token: &Arc<RwLock<Option<String>>>,
    server_url: &str,
) {
    let ui_weak = ui.as_weak();
    let handle = handle.clone();
    let auth_token = auth_token.clone();
    let server_url = server_url.to_string();
    ui.global::<AdminState>().on_business_type_delete(move |id| {
        let id = id.to_string();
        let ui_weak = ui_weak.clone();
        let auth_token = auth_token.clone();
        let server_url = server_url.clone();
        handle.spawn(async move {
            let Some(token) = auth_token.read().await.clone() else { return };
            let result = HTTP_CLIENT
                .delete(format!("{server_url}/admin/business-types/{id}"))
                .bearer_auth(&token)
                .send()
                .await;
            report(ui_weak, result, "Tipo de empresa removido").await;
        });
    });
}
