//! Perfil do usuário logado (modal do menu lateral). Online: busca os
//! dados em `GET /auth/me` ao abrir e salva em `PUT /auth/profile`.
//!
//! Regras (§1/§11): UI burra — a autoridade (validação/e-mail único/hash)
//! é o backend, que só deixa o operador editar a SI MESMO (via JWT).

use std::sync::Arc;

use serde::Deserialize;
use slint::{ComponentHandle, SharedString};
use tokio::sync::RwLock;

use crate::context::DesktopState;
use crate::{MainWindow, HTTP_CLIENT};

use super::super::helpers::show_toast;
use super::super::image::{decode_pixel_buffer, pick_image_file, process_image_file};

#[derive(Deserialize)]
struct MeDto {
    name: String,
    email: String,
    /// Foto de perfil (base64) ou ausente/null se não houver.
    #[serde(default)]
    avatar: Option<String>,
}

pub(crate) fn setup_profile(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    auth_token: Arc<RwLock<Option<String>>>,
    server_url: String,
) {
    // Abrir o perfil: mostra o modal e busca os dados atuais.
    {
        let ui_weak = ui.as_weak();
        let handle = handle.clone();
        let auth_token = auth_token.clone();
        let url = server_url.clone();
        ui.on_open_profile(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            ui.set_profile_status(SharedString::default());
            ui.set_profile_new_password(SharedString::default());
            ui.set_profile_open(true);
            let ui_weak = ui.as_weak();
            let auth_token = auth_token.clone();
            let url = url.clone();
            handle.spawn(async move {
                let Some(token) = auth_token.read().await.clone() else { return };
                let me: Option<MeDto> = match HTTP_CLIENT
                    .get(format!("{url}/auth/me"))
                    .bearer_auth(&token)
                    .send()
                    .await
                {
                    Ok(r) if r.status().is_success() => r.json().await.ok(),
                    _ => None,
                };
                // Decodifica a foto (base64 → pixels) fora do event loop.
                let avatar_b64 = me.as_ref().and_then(|m| m.avatar.clone())
                    .filter(|s| !s.is_empty());
                let pixel = match avatar_b64.clone() {
                    Some(b64) => tokio::task::spawn_blocking(move || decode_pixel_buffer(&b64))
                        .await
                        .unwrap_or(None),
                    None => None,
                };
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = ui_weak.upgrade() else { return };
                    if let Some(me) = me {
                        ui.set_profile_name(SharedString::from(me.name));
                        ui.set_profile_email(SharedString::from(me.email));
                        ui.set_profile_avatar(
                            pixel.map(slint::Image::from_rgba8).unwrap_or_default(),
                        );
                        ui.set_profile_avatar_data(SharedString::from(avatar_b64.unwrap_or_default()));
                    } else {
                        ui.set_profile_status(SharedString::from(
                            "Não foi possível carregar seus dados (offline?).",
                        ));
                    }
                });
            });
        });
    }
    // Salvar as credenciais.
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        let handle = handle.clone();
        let url = server_url;
        ui.on_profile_save(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            let name = ui.get_profile_name().trim().to_string();
            let email = ui.get_profile_email().trim().to_string();
            let password = ui.get_profile_new_password().to_string();
            let avatar = ui.get_profile_avatar_data().to_string();
            if name.is_empty() || email.is_empty() {
                ui.set_profile_status(SharedString::from("Informe nome e e-mail."));
                return;
            }
            ui.set_profile_busy(true);
            ui.set_profile_status(SharedString::from("Salvando..."));
            let ui_weak = ui.as_weak();
            let state = state.clone();
            let auth_token = auth_token.clone();
            let url = url.clone();
            handle.spawn(async move {
                let Some(token) = auth_token.read().await.clone() else { return };
                let pw = if password.trim().is_empty() { None } else { Some(password) };
                let res = HTTP_CLIENT
                    .put(format!("{url}/auth/profile"))
                    .bearer_auth(&token)
                    .json(&serde_json::json!({
                        "name": name, "email": email, "password": pw, "avatar": avatar,
                    }))
                    .send()
                    .await;
                let outcome: Result<(), String> = match res {
                    Ok(r) if r.status().is_success() => Ok(()),
                    Ok(r) => {
                        let body = r.text().await.unwrap_or_default();
                        Err(serde_json::from_str::<serde_json::Value>(&body)
                            .ok()
                            .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| "Não foi possível salvar o perfil.".into()))
                    }
                    Err(_) => Err("Sem conexão com o servidor.".into()),
                };
                // Nome atualizado sobrevive a restart offline (rodapé da sidebar).
                if outcome.is_ok() {
                    state.session.save_user_name(&name).await;
                }
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = ui_weak.upgrade() else { return };
                    ui.set_profile_busy(false);
                    match outcome {
                        Ok(()) => {
                            ui.set_user_name(SharedString::from(name));
                            ui.set_profile_new_password(SharedString::default());
                            ui.set_profile_open(false);
                            show_toast(&ui, "Perfil atualizado", "success");
                        }
                        Err(msg) => ui.set_profile_status(SharedString::from(msg)),
                    }
                });
            });
        });
    }
    // Trocar a foto de perfil: abre o seletor nativo, processa a imagem e
    // exibe na hora (a persistência ocorre ao clicar em Salvar).
    {
        let ui_weak = ui.as_weak();
        let handle = handle.clone();
        ui.on_profile_pick_avatar(move || {
            let ui_weak = ui_weak.clone();
            handle.spawn_blocking(move || {
                let Some(path) = pick_image_file() else { return };
                let Some(encoded) = process_image_file(&path) else {
                    tracing::error!("Falha ao processar avatar: {}", path.display());
                    return;
                };
                let pixel = decode_pixel_buffer(&encoded);
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = ui_weak.upgrade() else { return };
                    ui.set_profile_avatar(
                        pixel.map(slint::Image::from_rgba8).unwrap_or_default(),
                    );
                    ui.set_profile_avatar_data(SharedString::from(encoded));
                });
            });
        });
    }
}
