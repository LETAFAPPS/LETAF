use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use slint::{ComponentHandle, SharedString};
use tokio::sync::RwLock;
use uuid::Uuid;

use letaf_core::payment_gateway::model::{ChargeStatus, PaymentCharge};

use crate::context::DesktopState;
use crate::format::money_br;
use crate::HTTP_CLIENT;
use crate::{
    MainWindow, PixChargeView,
};

use super::super::helpers::show_toast;

// ── PIX (Efi) ────────────────────────────────────────────────────
//
// Regras aplicadas (AI_RULES.md §11):
// - O desktop não fala direto com a Efi. Toda chamada vai pro server
//   (que tem mTLS + OAuth + credenciais).
// - Polling enquanto o modal está aberto: 3s entre tentativas, para
//   no primeiro status terminal.

#[derive(Deserialize)]
struct ChargeResponse {
    charge: PaymentCharge,
}

#[derive(Serialize)]
struct CreateChargeRequest<'a> {
    invoice_id: Option<Uuid>,
    amount: f64,
    description: &'a str,
}

pub(crate) fn setup_pix_modal(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    auth_token: Arc<RwLock<Option<String>>>,
    server_url: String,
) {
    // Flag compartilhada: `true` enquanto o modal PIX está aberto.
    // Permite o polling parar sem precisar consultar o event loop.
    let modal_open = Arc::new(AtomicBool::new(false));

    // pay-invoice: cria a cobrança no server e abre o modal.
    let ui_weak = ui.as_weak();
    let state_pay = state.clone();
    let handle_pay = handle.clone();
    let auth_token_pay = auth_token.clone();
    let server_url_pay = server_url.clone();
    let modal_open_pay = modal_open.clone();
    ui.on_subscription_pay_invoice(move |invoice_id_str| {
        let Ok(invoice_id) = Uuid::parse_str(invoice_id_str.as_str()) else {
            return;
        };
        let ui_weak = ui_weak.clone();
        let state = state_pay.clone();
        let auth_token = auth_token_pay.clone();
        let server_url = server_url_pay.clone();
        let handle_for_poll = handle_pay.clone();
        let modal_open_inner = modal_open_pay.clone();

        modal_open_inner.store(true, Ordering::SeqCst);

        // Mostra modal em "loading" antes do server responder.
        let ui_weak_open = ui_weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_weak_open.upgrade() {
                ui.set_pix_charge_view(loading_view());
                ui.set_pix_modal_open(true);
            }
        });

        handle_pay.spawn(async move {
            let cid = state.company_id();
            let invoice = match state
                .subscription_service
                .find_invoices(cid)
                .await
                .ok()
                .and_then(|list| list.into_iter().find(|i| i.base.id == invoice_id))
            {
                Some(inv) => inv,
                None => {
                    set_error_view(&ui_weak, "Fatura não encontrada".to_string());
                    return;
                }
            };

            let token_opt = auth_token.read().await.clone();
            let Some(token) = token_opt else {
                set_error_view(&ui_weak, "Faça login para cobrar".to_string());
                return;
            };

            let body = CreateChargeRequest {
                invoice_id: Some(invoice.base.id),
                amount: invoice.amount,
                description: &format!("Fatura {} · LETAF", invoice.number),
            };
            let url = format!("{}/payments/pix/charge", server_url);
            let resp = match HTTP_CLIENT
                .post(&url)
                .bearer_auth(&token)
                .json(&body)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    set_error_view(&ui_weak, format!("Falha de rede: {e}"));
                    return;
                }
            };
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                set_error_view(
                    &ui_weak,
                    if status == reqwest::StatusCode::SERVICE_UNAVAILABLE {
                        "Gateway Efi não configurado no servidor".into()
                    } else {
                        format!("Erro {status}: {body}")
                    },
                );
                return;
            }
            let charge_resp: ChargeResponse = match resp.json().await {
                Ok(v) => v,
                Err(e) => {
                    set_error_view(&ui_weak, format!("Resposta inválida: {e}"));
                    return;
                }
            };

            apply_charge_view(&ui_weak, &charge_resp.charge);

            // Dispara polling em background até status terminal.
            spawn_polling(
                &handle_for_poll,
                ui_weak.clone(),
                state.clone(),
                auth_token.clone(),
                server_url.clone(),
                charge_resp.charge.base.id,
                invoice.base.id,
                modal_open_inner.clone(),
            );
        });
    });

    // copy-code: copia o copia-cola para o clipboard via xclip/wl-copy/etc.
    let ui_weak = ui.as_weak();
    ui.on_subscription_copy_pix_code(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        let view = ui.get_pix_charge_view();
        let code = view.pix_copia_cola.to_string();
        if code.is_empty() {
            return;
        }
        if write_clipboard(&code) {
            show_toast(&ui, "Código PIX copiado", "success");
        } else {
            show_toast(&ui, "Não foi possível copiar — selecione manualmente", "error");
        }
    });

    // close-pix-modal
    let ui_weak = ui.as_weak();
    let modal_open_close = modal_open.clone();
    ui.on_subscription_close_pix_modal(move || {
        modal_open_close.store(false, Ordering::SeqCst);
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_pix_modal_open(false);
            ui.set_pix_charge_view(empty_view());
            ui.set_pix_qr_image(slint::Image::default());
            ui.invoke_subscription_refresh();
        }
    });
}

// Os 8 parâmetros são handles distintos que o task de polling precisa
// (runtime, UI, estado, token, URL, ids e flag do modal); agrupar em
// struct não traria ganho — mantemos a assinatura direta (AI_RULES §8).
#[allow(clippy::too_many_arguments)]
fn spawn_polling(
    handle: &tokio::runtime::Handle,
    ui_weak: slint::Weak<MainWindow>,
    state: DesktopState,
    auth_token: Arc<RwLock<Option<String>>>,
    server_url: String,
    charge_id: Uuid,
    invoice_id: Uuid,
    modal_open: Arc<AtomicBool>,
) {
    handle.spawn(async move {
        // ~10 minutos no máximo (200 tentativas × 3s). O modal pode
        // fechar antes — paramos quando `modal_open` virar false.
        for _ in 0..200 {
            tokio::time::sleep(Duration::from_secs(3)).await;

            if !modal_open.load(Ordering::SeqCst) {
                return;
            }

            let token_opt = auth_token.read().await.clone();
            let Some(token) = token_opt else { return };

            let url = format!(
                "{}/payments/pix/charge/{}/refresh",
                server_url, charge_id
            );
            let resp = match HTTP_CLIENT.post(&url).bearer_auth(&token).send().await {
                Ok(r) => r,
                Err(_) => continue,
            };
            if !resp.status().is_success() {
                continue;
            }
            let Ok(parsed): Result<ChargeResponse, _> = resp.json().await else {
                continue;
            };

            if matches!(parsed.charge.status, ChargeStatus::Paid) {
                // Marca a fatura como Paga no banco local — `mark_invoice_paid`
                // é idempotente (se já estava paga, não muda nada). O
                // SyncWorker leva a mudança ao server.
                let cid = state.company_id();
                let paid_at = parsed.charge.paid_at;
                if let Err(e) = state
                    .subscription_service
                    .mark_invoice_paid(cid, invoice_id, paid_at)
                    .await
                {
                    tracing::warn!("mark_invoice_paid falhou: {e}");
                }
                let ui_weak = ui_weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak.upgrade() {
                        ui.set_pix_charge_view(paid_view());
                    }
                });
                return;
            }
            if matches!(
                parsed.charge.status,
                ChargeStatus::Failed | ChargeStatus::Cancelled | ChargeStatus::Expired
            ) {
                let msg = parsed
                    .charge
                    .last_error
                    .unwrap_or_else(|| "Cobrança encerrada sem pagamento".into());
                set_error_view(&ui_weak, msg);
                return;
            }
        }
    });
}

fn apply_charge_view(ui_weak: &slint::Weak<MainWindow>, c: &PaymentCharge) {
    let view = ready_view(c);
    // Decodifica o PNG fora do event loop (image::load_from_memory
    // pode levar alguns ms). O `SharedPixelBuffer` é Send.
    let buffer = c
        .qr_code_b64
        .as_deref()
        .and_then(super::super::image::decode_pixel_buffer);
    let ui_weak = ui_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_pix_charge_view(view);
            if let Some(buf) = buffer {
                ui.set_pix_qr_image(slint::Image::from_rgba8(buf));
            } else {
                ui.set_pix_qr_image(slint::Image::default());
            }
        }
    });
}

fn ready_view(c: &PaymentCharge) -> PixChargeView {
    PixChargeView {
        id: SharedString::from(c.base.id.to_string()),
        kind: SharedString::from("ready"),
        qr_code_b64: SharedString::from(c.qr_code_b64.clone().unwrap_or_default()),
        pix_copia_cola: SharedString::from(c.pix_copia_cola.clone().unwrap_or_default()),
        amount_display: SharedString::from(money_br(c.amount)),
        message: SharedString::default(),
    }
}

fn loading_view() -> PixChargeView {
    PixChargeView {
        id: SharedString::default(),
        kind: SharedString::from("loading"),
        qr_code_b64: SharedString::default(),
        pix_copia_cola: SharedString::default(),
        amount_display: SharedString::default(),
        message: SharedString::default(),
    }
}

pub(crate) fn paid_view() -> PixChargeView {
    PixChargeView {
        id: SharedString::default(),
        kind: SharedString::from("paid"),
        qr_code_b64: SharedString::default(),
        pix_copia_cola: SharedString::default(),
        amount_display: SharedString::default(),
        message: SharedString::default(),
    }
}

fn empty_view() -> PixChargeView {
    PixChargeView {
        id: SharedString::default(),
        kind: SharedString::default(),
        qr_code_b64: SharedString::default(),
        pix_copia_cola: SharedString::default(),
        amount_display: SharedString::default(),
        message: SharedString::default(),
    }
}

pub(crate) fn set_error_view(ui_weak: &slint::Weak<MainWindow>, message: String) {
    let view = PixChargeView {
        id: SharedString::default(),
        kind: SharedString::from("error"),
        qr_code_b64: SharedString::default(),
        pix_copia_cola: SharedString::default(),
        amount_display: SharedString::default(),
        message: SharedString::from(message),
    };
    let ui_weak = ui_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_pix_charge_view(view);
        }
    });
}

/// Best-effort: tenta xclip → wl-copy (Wayland) → xsel. Sem nada
/// instalado retorna false (toast informa).
fn write_clipboard(text: &str) -> bool {
    let cmds = [
        ("xclip", vec!["-selection", "clipboard"]),
        ("wl-copy", vec![]),
        ("xsel", vec!["--clipboard", "--input"]),
    ];
    for (cmd, args) in &cmds {
        if let Ok(mut child) = std::process::Command::new(cmd)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            if let Some(stdin) = child.stdin.as_mut() {
                use std::io::Write;
                if stdin.write_all(text.as_bytes()).is_ok() && child.wait().is_ok() {
                    return true;
                }
            }
        }
    }
    false
}

