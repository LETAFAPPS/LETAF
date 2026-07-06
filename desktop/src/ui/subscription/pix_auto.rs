use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use slint::{ComponentHandle, SharedString};
use tokio::sync::{Notify, RwLock};

use letaf_core::subscription::model::Subscription;

use crate::context::DesktopState;
use crate::format::format_document;
use crate::HTTP_CLIENT;
use crate::{
    MainWindow, PixChargeView,
};

use super::super::helpers::show_toast;
use super::pix::{paid_view, set_error_view};
use super::card::{refresh, toast};

// ── Pix Automático (mandato de débito recorrente) ────────────────
//
// Regras aplicadas (AI_RULES.md §11):
// - O desktop não fala com a Efi; o mandato é criado no server.
// - Coleta nome + CPF → server gera a recorrência e devolve o QR de
//   autorização, exibido no modal PIX. Polling até o mandato ativar
//   (o pagador autoriza no app do banco dele).

#[derive(Serialize)]
struct PixAutoActivateBody {
    customer_name: String,
    customer_cpf: String,
}

#[derive(Deserialize)]
struct PixAutoActivateResp {
    subscription: Subscription,
    copia_cola: String,
    qr_code_b64: String,
}

pub(crate) fn setup_pix_auto(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    auth_token: Arc<RwLock<Option<String>>>,
    server_url: String,
    sync_notify: Arc<Notify>,
) {
    ui.on_pix_auto_fmt_cpf_cnpj(|raw| SharedString::from(format_document(raw.as_str())));

    // Abre o modal de ativação.
    let ui_weak = ui.as_weak();
    ui.on_subscription_open_pix_auto(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_pix_auto_name(SharedString::default());
            ui.set_pix_auto_cpf(SharedString::default());
            ui.set_pix_auto_error(SharedString::default());
            ui.set_pix_auto_loading(false);
            ui.set_pix_auto_modal_open(true);
        }
    });

    let ui_weak = ui.as_weak();
    ui.on_pix_auto_close(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_pix_auto_modal_open(false);
        }
    });

    // Ativa: cria a recorrência no server e mostra o QR de autorização.
    let ui_weak = ui.as_weak();
    let state_a = state.clone();
    let handle_a = handle.clone();
    let auth_a = auth_token.clone();
    let url_a = server_url.clone();
    let notify_a = sync_notify.clone();
    ui.on_pix_auto_submit(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        let name = ui.get_pix_auto_name().to_string().trim().to_string();
        let cpf_digits: String = ui
            .get_pix_auto_cpf()
            .chars()
            .filter(|c| c.is_ascii_digit())
            .collect();
        if name.is_empty() {
            ui.set_pix_auto_error(SharedString::from("Informe o nome do titular"));
            return;
        }
        if cpf_digits.len() != 11 && cpf_digits.len() != 14 {
            ui.set_pix_auto_error(SharedString::from("CPF (11) ou CNPJ (14) inválido"));
            return;
        }
        ui.set_pix_auto_error(SharedString::default());
        ui.set_pix_auto_loading(true);
        let body = PixAutoActivateBody {
            customer_name: name,
            customer_cpf: cpf_digits,
        };
        let ui_weak = ui_weak.clone();
        let state = state_a.clone();
        let auth = auth_a.clone();
        let url = url_a.clone();
        let notify = notify_a.clone();
        let handle_poll = handle_a.clone();
        handle_a.spawn(async move {
            let cid = state.company_id();
            let Some(token) = auth.read().await.clone() else {
                pix_auto_form_error(&ui_weak, "Faça login para ativar".into());
                return;
            };
            let endpoint = format!("{}/subscription/pix-auto", url);
            let resp = HTTP_CLIENT
                .post(&endpoint)
                .bearer_auth(&token)
                .json(&body)
                .send()
                .await;
            let resp = match resp {
                Ok(r) => r,
                Err(e) => {
                    pix_auto_form_error(&ui_weak, format!("Falha de rede: {e}"));
                    return;
                }
            };
            if !resp.status().is_success() {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                pix_auto_form_error(
                    &ui_weak,
                    if status == reqwest::StatusCode::SERVICE_UNAVAILABLE {
                        "Pix Automático não configurado no servidor".into()
                    } else {
                        format!("Erro {status}: {text}")
                    },
                );
                return;
            }
            let parsed: PixAutoActivateResp = match resp.json().await {
                Ok(v) => v,
                Err(e) => {
                    pix_auto_form_error(&ui_weak, format!("Resposta inválida: {e}"));
                    return;
                }
            };
            let _ = state
                .subscription_service
                .sync_upsert_subscription(cid, parsed.subscription)
                .await;
            notify.notify_one();
            // Fecha o form e mostra o QR de autorização no modal PIX.
            show_pix_auth_qr(&ui_weak, &parsed.copia_cola, &parsed.qr_code_b64);
            spawn_pix_auto_polling(&handle_poll, ui_weak.clone(), state.clone(), auth.clone(), url.clone(), notify.clone());
        });
    });

    // Cancela o mandato.
    let ui_weak = ui.as_weak();
    let state_c = state.clone();
    let handle_c = handle.clone();
    let auth_c = auth_token;
    let url_c = server_url;
    let notify_c = sync_notify;
    ui.on_subscription_cancel_pix_auto(move || {
        let ui_weak = ui_weak.clone();
        let state = state_c.clone();
        let auth = auth_c.clone();
        let url = url_c.clone();
        let notify = notify_c.clone();
        handle_c.spawn(async move {
            let cid = state.company_id();
            let Some(token) = auth.read().await.clone() else {
                toast(&ui_weak, "Faça login para cancelar".into(), "error");
                return;
            };
            let endpoint = format!("{}/subscription/pix-auto", url);
            match HTTP_CLIENT.delete(&endpoint).bearer_auth(&token).send().await {
                Ok(r) if r.status().is_success() => {
                    if let Ok(sub) = r.json::<Subscription>().await {
                        let _ = state
                            .subscription_service
                            .sync_upsert_subscription(cid, sub)
                            .await;
                    }
                    notify.notify_one();
                    toast(&ui_weak, "PIX Automático cancelado".into(), "success");
                    refresh(&ui_weak);
                }
                Ok(r) => {
                    let s = r.status();
                    toast(&ui_weak, format!("Erro ao cancelar ({s})"), "error");
                }
                Err(e) => toast(&ui_weak, format!("Falha de rede: {e}"), "error"),
            }
        });
    });
}

fn pix_auto_form_error(ui_weak: &slint::Weak<MainWindow>, message: String) {
    let ui_weak = ui_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_pix_auto_loading(false);
            ui.set_pix_auto_error(SharedString::from(message));
        }
    });
}

/// Mostra o QR de **autorização** do mandato no modal PIX reaproveitado.
fn show_pix_auth_qr(ui_weak: &slint::Weak<MainWindow>, copia_cola: &str, qr_b64: &str) {
    let view = PixChargeView {
        id: SharedString::default(),
        kind: SharedString::from("ready"),
        qr_code_b64: SharedString::from(qr_b64),
        pix_copia_cola: SharedString::from(copia_cola),
        amount_display: SharedString::default(),
        message: SharedString::from(
            "Escaneie no app do seu banco para autorizar o débito automático",
        ),
    };
    let buffer = super::super::image::decode_pixel_buffer(qr_b64);
    let ui_weak = ui_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_pix_auto_loading(false);
            ui.set_pix_auto_modal_open(false);
            ui.set_pix_charge_view(view);
            if let Some(buf) = buffer {
                ui.set_pix_qr_image(slint::Image::from_rgba8(buf));
            } else {
                ui.set_pix_qr_image(slint::Image::default());
            }
            ui.set_pix_modal_open(true);
        }
    });
}

/// Polling do status do mandato após o pagador escanear o QR. Quando
/// ativo, atualiza local + marca sucesso. ~5 min no máximo.
fn spawn_pix_auto_polling(
    handle: &tokio::runtime::Handle,
    ui_weak: slint::Weak<MainWindow>,
    state: DesktopState,
    auth_token: Arc<RwLock<Option<String>>>,
    server_url: String,
    sync_notify: Arc<Notify>,
) {
    handle.spawn(async move {
        for _ in 0..100 {
            tokio::time::sleep(Duration::from_secs(3)).await;
            let Some(token) = auth_token.read().await.clone() else { return };
            let url = format!("{}/subscription/pix-auto/status", server_url);
            let resp = match HTTP_CLIENT.get(&url).bearer_auth(&token).send().await {
                Ok(r) => r,
                Err(_) => continue,
            };
            if !resp.status().is_success() {
                continue;
            }
            let Ok(sub): Result<Subscription, _> = resp.json().await else { continue };
            let active = sub.has_active_pix_auto();
            let rejected = matches!(sub.pix_auto_status.as_deref(), Some("rejected") | Some("canceled"));
            let cid = state.company_id();
            let _ = state
                .subscription_service
                .sync_upsert_subscription(cid, sub)
                .await;
            if active {
                sync_notify.notify_one();
                let ui_weak = ui_weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak.upgrade() {
                        ui.set_pix_charge_view(paid_view());
                        show_toast(&ui, "PIX Automático autorizado · débito ativo", "success");
                        ui.invoke_subscription_refresh();
                    }
                });
                return;
            }
            if rejected {
                set_error_view(&ui_weak, "Autorização recusada ou cancelada".into());
                return;
            }
        }
    });
}

