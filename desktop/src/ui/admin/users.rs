//! Usuários do painel (administradores): formulário e persistência.

use std::sync::Arc;

use slint::{ComponentHandle, Model, SharedString};
use tokio::sync::RwLock;

use crate::{AdminState, MainWindow, HTTP_CLIENT};

use super::super::helpers::show_toast;
use super::http::{report, report_modal};

/// Callbacks síncronos do formulário (novo / editar → preenche campos).
pub(super) fn setup_form(ui: &MainWindow) {
    // Novo: limpa o formulário.
    {
        let ui_weak = ui.as_weak();
        ui.global::<AdminState>().on_new_user(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            ui.global::<AdminState>().set_form_id(SharedString::new());
            ui.global::<AdminState>().set_form_name(SharedString::new());
            ui.global::<AdminState>().set_form_email(SharedString::new());
            ui.global::<AdminState>().set_form_password(SharedString::new());
            ui.global::<AdminState>().set_user_form_role_id(SharedString::new());
            ui.global::<AdminState>().set_user_modal_open(true);
        });
    }
    // Editar: acha o admin no modelo e preenche (senha em branco = manter).
    {
        let ui_weak = ui.as_weak();
        ui.global::<AdminState>().on_edit_user(move |id| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let users = ui.global::<AdminState>().get_users();
            if let Some(u) = users.iter().find(|u| u.id == id) {
                ui.global::<AdminState>().set_form_id(u.id.clone());
                ui.global::<AdminState>().set_form_name(u.name.clone());
                ui.global::<AdminState>().set_form_email(u.email.clone());
                ui.global::<AdminState>().set_form_password(SharedString::new());
                ui.global::<AdminState>().set_user_form_role_id(u.role_id.clone());
                ui.global::<AdminState>().set_user_modal_open(true);
            }
        });
    }
}

/// Salvar (criar/atualizar), excluir e ativar/desativar administrador.
pub(super) fn setup_persist(
    ui: &MainWindow,
    handle: &tokio::runtime::Handle,
    auth_token: &Arc<RwLock<Option<String>>>,
    server_url: &str,
) {
    setup_save_user(ui, handle, auth_token, server_url);
    setup_delete_user(ui, handle, auth_token, server_url);
    setup_user_active(ui, handle, auth_token, server_url);
}

/// Dados do administrador enviados ao servidor.
struct UserPayload {
    id: String,
    name: String,
    email: String,
    password: String,
    /// Função escolhida (id do catálogo). `None` = master.
    role_id: Option<String>,
}

/// Salvar.
fn setup_save_user(
    ui: &MainWindow,
    handle: &tokio::runtime::Handle,
    auth_token: &Arc<RwLock<Option<String>>>,
    server_url: &str,
) {
    let ui_weak = ui.as_weak();
    let handle = handle.clone();
    let auth_token = auth_token.clone();
    let server_url = server_url.to_string();
    ui.global::<AdminState>().on_save_user(move |id, name, email, password| {
        let name = name.trim().to_string();
        let email = email.trim().to_string();
        if !user_form_valid(&ui_weak, &id, &name, &email, &password) {
            return;
        }
        // Função escolhida (id do catálogo). Vazio = master.
        let role_id = ui_weak
            .upgrade()
            .map(|ui| ui.global::<AdminState>().get_user_form_role_id().to_string())
            .unwrap_or_default();
        let payload = UserPayload {
            id: id.to_string(),
            name,
            email,
            password: password.to_string(),
            role_id: if role_id.trim().is_empty() { None } else { Some(role_id) },
        };
        let ui_weak = ui_weak.clone();
        let auth_token = auth_token.clone();
        let server_url = server_url.clone();
        handle.spawn(async move {
            let Some(token) = auth_token.read().await.clone() else { return };
            let result = send_user(&server_url, &token, &payload).await;
            report_modal(ui_weak, result, "Administrador Salvo", |ui| {
                ui.global::<AdminState>().set_user_modal_open(false);
            })
            .await;
        });
    });
}

/// `true` se o formulário pode ser enviado; senão mostra o toast do erro.
fn user_form_valid(
    ui_weak: &slint::Weak<MainWindow>,
    id: &str,
    name: &str,
    email: &str,
    password: &str,
) -> bool {
    if name.is_empty() || email.is_empty() {
        if let Some(ui) = ui_weak.upgrade() {
            show_toast(&ui, "Informe nome e e-mail", "error");
        }
        return false;
    }
    if id.is_empty() && password.trim().is_empty() {
        if let Some(ui) = ui_weak.upgrade() {
            show_toast(&ui, "Defina uma senha para o novo administrador", "error");
        }
        return false;
    }
    true
}

/// POST (novo) ou PUT (edição — senha vazia mantém a atual).
async fn send_user(
    server_url: &str,
    token: &str,
    p: &UserPayload,
) -> Result<reqwest::Response, reqwest::Error> {
    if p.id.is_empty() {
        let body = serde_json::json!({ "name": p.name, "email": p.email, "password": p.password, "admin_role_id": p.role_id });
        HTTP_CLIENT
            .post(format!("{server_url}/admin/admins"))
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
    } else {
        let pw = if p.password.trim().is_empty() { None } else { Some(p.password.as_str()) };
        let body = serde_json::json!({ "name": p.name, "email": p.email, "password": pw, "admin_role_id": p.role_id });
        HTTP_CLIENT
            .put(format!("{server_url}/admin/admins/{}", p.id))
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
    }
}

/// Excluir.
fn setup_delete_user(
    ui: &MainWindow,
    handle: &tokio::runtime::Handle,
    auth_token: &Arc<RwLock<Option<String>>>,
    server_url: &str,
) {
    let ui_weak = ui.as_weak();
    let handle = handle.clone();
    let auth_token = auth_token.clone();
    let server_url = server_url.to_string();
    ui.global::<AdminState>().on_delete_user(move |id| {
        let id = id.to_string();
        let ui_weak = ui_weak.clone();
        let auth_token = auth_token.clone();
        let server_url = server_url.clone();
        handle.spawn(async move {
            let Some(token) = auth_token.read().await.clone() else { return };
            let result = HTTP_CLIENT
                .delete(format!("{server_url}/admin/admins/{id}"))
                .bearer_auth(&token)
                .send()
                .await;
            report(ui_weak, result, "Administrador Removido").await;
        });
    });
}

/// Ativar/desativar o acesso de um administrador (o master é barrado no
/// backend — §11). PUT /admin/admins/{id}/active.
fn setup_user_active(
    ui: &MainWindow,
    handle: &tokio::runtime::Handle,
    auth_token: &Arc<RwLock<Option<String>>>,
    server_url: &str,
) {
    let ui_weak = ui.as_weak();
    let handle = handle.clone();
    let auth_token = auth_token.clone();
    let server_url = server_url.to_string();
    ui.global::<AdminState>().on_set_user_active(move |id, active| {
        let id = id.to_string();
        if id.is_empty() {
            return;
        }
        let ui_weak = ui_weak.clone();
        let auth_token = auth_token.clone();
        let server_url = server_url.clone();
        handle.spawn(async move {
            let Some(token) = auth_token.read().await.clone() else { return };
            let result = HTTP_CLIENT
                .put(format!("{server_url}/admin/admins/{id}/active"))
                .bearer_auth(&token)
                .json(&serde_json::json!({ "active": active }))
                .send()
                .await;
            let msg = if active { "Usuário Ativado" } else { "Usuário Desativado" };
            report(ui_weak, result, msg).await;
        });
    });
}
