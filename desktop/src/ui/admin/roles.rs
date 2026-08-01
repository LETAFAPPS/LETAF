//! Funções do painel (conjunto de telas que um administrador enxerga).
//!
//! A autoridade continua no backend (§11): aqui só montamos o formulário e
//! chamamos `/admin/roles`.

use std::sync::Arc;

use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use tokio::sync::RwLock;

use crate::{AdminScreenOption, AdminState, MainWindow, HTTP_CLIENT};

use super::super::helpers::show_toast;
use super::cache::RolesCache;
use super::filters::apply_role_filter;
use super::http::{report, report_modal};

/// Catálogo de TELAS do painel (espelha `core::admin_role::model::SCREENS`).
/// Fonte para os checkboxes do modal e o resumo no card de Função.
const ADMIN_SCREENS: &[(&str, &str)] = &[
    ("overview", "Dashboard"),
    ("companies", "Empresas"),
    ("plans", "Planos"),
    ("admins", "Usuários"),
    ("roles", "Funções"),
];

pub(super) fn screen_label(key: &str) -> &'static str {
    ADMIN_SCREENS
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, l)| *l)
        .unwrap_or("?")
}

/// Monta as opções de tela (checkbox) do formulário de Função, marcando as
/// telas em `selected`.
fn build_screen_options(selected: &[String]) -> Vec<AdminScreenOption> {
    ADMIN_SCREENS
        .iter()
        .map(|(key, label)| AdminScreenOption {
            key: (*key).into(),
            label: (*label).into(),
            on: selected.iter().any(|s| s == key),
        })
        .collect()
}

/// Callbacks das Funções: form (novo/editar/toggle), salvar/excluir e busca.
pub(super) fn setup_roles(
    ui: &MainWindow,
    handle: &tokio::runtime::Handle,
    auth_token: &Arc<RwLock<Option<String>>>,
    server_url: &str,
    roles_cache: &RolesCache,
) {
    setup_role_new(ui);
    setup_role_edit(ui, roles_cache);
    setup_role_toggle_screen(ui);
    setup_role_save(ui, handle, auth_token, server_url);
    setup_role_delete(ui, handle, auth_token, server_url);
    setup_role_search(ui, roles_cache);
}

/// Novo: form limpo + telas todas desmarcadas.
fn setup_role_new(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.global::<AdminState>().on_role_new(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        let g = ui.global::<AdminState>();
        g.set_role_form_id(SharedString::new());
        g.set_role_form_name(SharedString::new());
        g.set_role_form_screens(ModelRc::new(VecModel::from(build_screen_options(&[]))));
        g.set_role_modal_open(true);
    });
}

/// Editar: preenche do cache e marca as telas da função.
fn setup_role_edit(ui: &MainWindow, roles_cache: &RolesCache) {
    let ui_weak = ui.as_weak();
    let cache = roles_cache.clone();
    ui.global::<AdminState>().on_role_edit(move |id| {
        let Some(ui) = ui_weak.upgrade() else { return };
        let Ok(all) = cache.lock() else { return };
        if let Some(r) = all.iter().find(|r| r.id == id.as_str()) {
            let g = ui.global::<AdminState>();
            g.set_role_form_id(r.id.clone().into());
            g.set_role_form_name(r.name.clone().into());
            g.set_role_form_screens(ModelRc::new(VecModel::from(build_screen_options(&r.screens))));
            g.set_role_modal_open(true);
        }
    });
}

/// Alterna uma tela no formulário.
fn setup_role_toggle_screen(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.global::<AdminState>().on_role_toggle_screen(move |key| {
        let Some(ui) = ui_weak.upgrade() else { return };
        let model = ui.global::<AdminState>().get_role_form_screens();
        let toggled: Vec<AdminScreenOption> = (0..model.row_count())
            .filter_map(|i| model.row_data(i))
            .map(|mut o| {
                if o.key == key {
                    o.on = !o.on;
                }
                o
            })
            .collect();
        ui.global::<AdminState>().set_role_form_screens(ModelRc::new(VecModel::from(toggled)));
    });
}

/// Salvar (criar/atualizar).
fn setup_role_save(
    ui: &MainWindow,
    handle: &tokio::runtime::Handle,
    auth_token: &Arc<RwLock<Option<String>>>,
    server_url: &str,
) {
    let ui_weak = ui.as_weak();
    let handle = handle.clone();
    let auth_token = auth_token.clone();
    let server_url = server_url.to_string();
    ui.global::<AdminState>().on_role_save(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        let g = ui.global::<AdminState>();
        let id = g.get_role_form_id().to_string();
        let name = g.get_role_form_name().trim().to_string();
        if name.is_empty() {
            show_toast(&ui, "Informe o nome da função", "error");
            return;
        }
        let screens = selected_screens(&ui);
        if screens.is_empty() {
            show_toast(&ui, "Selecione ao menos uma tela", "error");
            return;
        }
        let body = serde_json::json!({ "name": name, "screens": screens });
        let ui_weak = ui.as_weak();
        let auth_token = auth_token.clone();
        let server_url = server_url.clone();
        handle.spawn(async move {
            let Some(token) = auth_token.read().await.clone() else { return };
            let result = if id.is_empty() {
                HTTP_CLIENT.post(format!("{server_url}/admin/roles")).bearer_auth(&token).json(&body).send().await
            } else {
                HTTP_CLIENT.put(format!("{server_url}/admin/roles/{id}")).bearer_auth(&token).json(&body).send().await
            };
            report_modal(ui_weak, result, "Função Salva", |ui| {
                ui.global::<AdminState>().set_role_modal_open(false);
            })
            .await;
        });
    });
}

/// Telas marcadas no formulário de Função.
fn selected_screens(ui: &MainWindow) -> Vec<String> {
    let model = ui.global::<AdminState>().get_role_form_screens();
    (0..model.row_count())
        .filter_map(|i| model.row_data(i))
        .filter(|o| o.on)
        .map(|o| o.key.to_string())
        .collect()
}

/// Excluir (acionado pela confirmação).
fn setup_role_delete(
    ui: &MainWindow,
    handle: &tokio::runtime::Handle,
    auth_token: &Arc<RwLock<Option<String>>>,
    server_url: &str,
) {
    let ui_weak = ui.as_weak();
    let handle = handle.clone();
    let auth_token = auth_token.clone();
    let server_url = server_url.to_string();
    ui.global::<AdminState>().on_role_delete(move |id| {
        let id = id.to_string();
        let ui_weak = ui_weak.clone();
        let auth_token = auth_token.clone();
        let server_url = server_url.clone();
        handle.spawn(async move {
            let Some(token) = auth_token.read().await.clone() else { return };
            let result = HTTP_CLIENT
                .delete(format!("{server_url}/admin/roles/{id}"))
                .bearer_auth(&token)
                .send()
                .await;
            report(ui_weak, result, "Função Removida").await;
        });
    });
}

/// Busca.
fn setup_role_search(ui: &MainWindow, roles_cache: &RolesCache) {
    let ui_weak = ui.as_weak();
    let cache = roles_cache.clone();
    ui.global::<AdminState>().on_filter_roles(move || {
        if let Some(ui) = ui_weak.upgrade() {
            apply_role_filter(&ui, &cache);
        }
    });
}
