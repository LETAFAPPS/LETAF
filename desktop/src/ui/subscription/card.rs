use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use slint::{ComponentHandle, SharedString};
use tokio::sync::{Notify, RwLock};

use letaf_core::subscription::model::Subscription;

use crate::context::DesktopState;
use crate::format::{
    format_card_expiry, format_card_number, format_cvv, format_date_br, format_document,
    format_phone,
};
use crate::HTTP_CLIENT;
use crate::MainWindow;

use super::super::helpers::show_toast;

// ── Cartão recorrente (cobrança automática via gateway) ──────────
//
// Regras aplicadas (AI_RULES.md §11):
// - O desktop NÃO fala direto com a Efi. O cartão vai para o server,
//   que tokeniza (mTLS/OAuth + credenciais) e cria a assinatura no
//   gateway. Aqui só coletamos e roteamos.
// - Os campos do cartão são limpos ao fechar — nada sensível persiste
//   no desktop.

#[derive(Serialize)]
struct CardSubscribeBody {
    number: String,
    holder_name: String,
    expiry: String,
    cvv: String,
    brand: String,
    cpf: String,
    email: String,
    phone: String,
    birth: String,
}

#[derive(Deserialize)]
struct CardSessionResp {
    session_token: String,
}

#[derive(Deserialize)]
struct CardSessionStatusResp {
    status: String,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Deserialize)]
struct SubscriptionResp {
    subscription: Option<Subscription>,
}

/// Abre uma URL no navegador padrão do sistema (Linux: `xdg-open`).
fn open_in_browser(url: &str) -> bool {
    std::process::Command::new("xdg-open")
        .arg(url)
        .spawn()
        .is_ok()
}

pub(crate) fn setup_card(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    auth_token: Arc<RwLock<Option<String>>>,
    server_url: String,
    sync_notify: Arc<Notify>,
) {
    // Máscaras de input (reaproveitam os formatadores do `format.rs`).
    ui.on_card_fmt_number(|raw| SharedString::from(format_card_number(raw.as_str())));
    ui.on_card_fmt_expiry(|raw| SharedString::from(format_card_expiry(raw.as_str())));
    ui.on_card_fmt_cvv(|raw| SharedString::from(format_cvv(raw.as_str())));
    ui.on_card_fmt_cpf_cnpj(|raw| SharedString::from(format_document(raw.as_str())));
    ui.on_card_fmt_birth(|raw| SharedString::from(format_date_br(raw.as_str())));
    ui.on_card_fmt_phone(|raw| SharedString::from(format_phone(raw.as_str())));

    // "Adicionar cartão": a tokenização é client-side (Efi.js, página
    // hosted). Abrimos uma sessão no server, abrimos o navegador na
    // página de cadastro e fazemos polling do status.
    let ui_weak = ui.as_weak();
    let state_open = state.clone();
    let handle_open = handle.clone();
    let auth_open = auth_token.clone();
    let url_open = server_url.clone();
    let notify_open = sync_notify.clone();
    ui.on_subscription_open_card_modal(move || {
        let ui_weak = ui_weak.clone();
        let state = state_open.clone();
        let auth = auth_open.clone();
        let url = url_open.clone();
        let notify = notify_open.clone();
        handle_open.spawn(async move {
            let Some(token) = auth.read().await.clone() else {
                toast(&ui_weak, "Faça login para cadastrar o cartão".into(), "error");
                return;
            };
            // 1) Abre a sessão de cadastro.
            let resp = HTTP_CLIENT
                .post(format!("{}/subscription/card/session", url))
                .bearer_auth(&token)
                .send()
                .await;
            let resp = match resp {
                Ok(r) => r,
                Err(e) => {
                    toast(&ui_weak, format!("Falha de rede: {e}"), "error");
                    return;
                }
            };
            if !resp.status().is_success() {
                let s = resp.status();
                toast(
                    &ui_weak,
                    if s == reqwest::StatusCode::SERVICE_UNAVAILABLE {
                        "Gateway de cartão não configurado no servidor".into()
                    } else {
                        format!("Erro ao abrir cadastro ({s})")
                    },
                    "error",
                );
                return;
            }
            let session = match resp.json::<CardSessionResp>().await {
                Ok(v) => v.session_token,
                Err(e) => {
                    toast(&ui_weak, format!("Resposta inválida: {e}"), "error");
                    return;
                }
            };
            // 2) Abre o navegador na página de cadastro (Efi.js).
            let page = format!("{}/pay/card?s={}", url, session);
            if open_in_browser(&page) {
                toast(&ui_weak, "Abri o navegador para cadastrar o cartão. Aguardando…".into(), "info");
            } else {
                toast(&ui_weak, format!("Abra no navegador: {page}"), "info");
            }
            // 3) Polling do status da sessão (~6 min).
            let cid = state.company_id();
            for _ in 0..120 {
                tokio::time::sleep(Duration::from_secs(3)).await;
                let st = HTTP_CLIENT
                    .get(format!("{}/subscription/card/session?s={}", url, session))
                    .bearer_auth(&token)
                    .send()
                    .await;
                let Ok(st) = st else { continue };
                if !st.status().is_success() {
                    continue;
                }
                let Ok(sv): Result<CardSessionStatusResp, _> = st.json().await else { continue };
                match sv.status.as_str() {
                    "completed" => {
                        // Reflete a assinatura do server (cartão vinculado).
                        if let Ok(r) = HTTP_CLIENT
                            .get(format!("{}/subscription", url))
                            .bearer_auth(&token)
                            .send()
                            .await
                        {
                            if let Ok(view) = r.json::<SubscriptionResp>().await {
                                if let Some(sub) = view.subscription {
                                    let _ = state
                                        .subscription_service
                                        .sync_upsert_subscription(cid, sub)
                                        .await;
                                }
                            }
                        }
                        notify.notify_one();
                        toast(&ui_weak, "Cartão cadastrado · cobrança automática ativa".into(), "success");
                        refresh(&ui_weak);
                        return;
                    }
                    "failed" => {
                        toast(
                            &ui_weak,
                            format!("Falha no cadastro do cartão: {}", sv.error.unwrap_or_default()),
                            "error",
                        );
                        return;
                    }
                    "expired" => return,
                    _ => {}
                }
            }
        });
    });

    // Fecha o modal e descarta os dados sensíveis.
    let ui_weak = ui.as_weak();
    ui.on_card_modal_close(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_card_modal_open(false);
            clear_card_form(&ui);
        }
    });

    // Cadastra o cartão: envia ao server e reflete o resultado local.
    let ui_weak = ui.as_weak();
    let state_add = state.clone();
    let handle_add = handle.clone();
    let auth_add = auth_token.clone();
    let url_add = server_url.clone();
    let notify_add = sync_notify.clone();
    ui.on_card_modal_submit(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        ui.set_card_form_error(SharedString::default());
        clear_card_errors(&ui);
        // Validação por campo (mensagens abaixo de cada campo). Só
        // envia quando tudo está válido.
        let body = match validate_card_form(&ui) {
            Ok(b) => b,
            Err(errs) => {
                apply_card_errors(&ui, &errs);
                return;
            }
        };
        ui.set_card_modal_loading(true);
        let ui_weak = ui_weak.clone();
        let state = state_add.clone();
        let auth = auth_add.clone();
        let url = url_add.clone();
        let notify = notify_add.clone();
        handle_add.spawn(async move {
            let cid = state.company_id();
            let token_opt = auth.read().await.clone();
            let Some(token) = token_opt else {
                set_card_error(&ui_weak, "Faça login para cadastrar o cartão".into());
                return;
            };
            let endpoint = format!("{}/subscription/card", url);
            let resp = HTTP_CLIENT
                .post(&endpoint)
                .bearer_auth(&token)
                .json(&body)
                .send()
                .await;
            let resp = match resp {
                Ok(r) => r,
                Err(e) => {
                    set_card_error(&ui_weak, format!("Falha de rede: {e}"));
                    return;
                }
            };
            if !resp.status().is_success() {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                set_card_error(
                    &ui_weak,
                    if status == reqwest::StatusCode::SERVICE_UNAVAILABLE {
                        "Gateway de cartão não configurado no servidor".into()
                    } else {
                        format!("Erro {status}: {text}")
                    },
                );
                return;
            }
            // Reflete a assinatura retornada localmente (server é a fonte
            // da verdade — `sync_upsert` aplica por last-write-wins).
            if let Ok(sub) = resp.json::<Subscription>().await {
                let _ = state
                    .subscription_service
                    .sync_upsert_subscription(cid, sub)
                    .await;
            }
            notify.notify_one();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    ui.set_card_modal_loading(false);
                    ui.set_card_modal_open(false);
                    clear_card_form(&ui);
                    show_toast(&ui, "Cartão cadastrado · cobrança automática ativa", "success");
                    ui.invoke_subscription_refresh();
                }
            });
        });
    });

    // Cancela o cartão recorrente atual.
    let ui_weak = ui.as_weak();
    let state_cancel = state.clone();
    let handle_cancel = handle.clone();
    let auth_cancel = auth_token;
    let url_cancel = server_url;
    let notify_cancel = sync_notify;
    ui.on_subscription_cancel_card(move || {
        let ui_weak = ui_weak.clone();
        let state = state_cancel.clone();
        let auth = auth_cancel.clone();
        let url = url_cancel.clone();
        let notify = notify_cancel.clone();
        handle_cancel.spawn(async move {
            let cid = state.company_id();
            let token_opt = auth.read().await.clone();
            let Some(token) = token_opt else {
                toast(&ui_weak, "Faça login para cancelar o cartão".into(), "error");
                return;
            };
            let endpoint = format!("{}/subscription/card", url);
            let resp = HTTP_CLIENT.delete(&endpoint).bearer_auth(&token).send().await;
            match resp {
                Ok(r) if r.status().is_success() => {
                    if let Ok(sub) = r.json::<Subscription>().await {
                        let _ = state
                            .subscription_service
                            .sync_upsert_subscription(cid, sub)
                            .await;
                    }
                    notify.notify_one();
                    toast(&ui_weak, "Cartão cancelado · cobrança voltou para PIX".into(), "success");
                    refresh(&ui_weak);
                }
                Ok(r) => {
                    let status = r.status();
                    toast(&ui_weak, format!("Erro ao cancelar ({status})"), "error");
                }
                Err(e) => toast(&ui_weak, format!("Falha de rede: {e}"), "error"),
            }
        });
    });
}

fn clear_card_form(ui: &MainWindow) {
    ui.set_card_form_number(SharedString::default());
    ui.set_card_form_holder(SharedString::default());
    ui.set_card_form_expiry(SharedString::default());
    ui.set_card_form_cvv(SharedString::default());
    ui.set_card_form_cpf(SharedString::default());
    ui.set_card_form_email(SharedString::default());
    ui.set_card_form_phone(SharedString::default());
    ui.set_card_form_birth(SharedString::default());
    ui.set_card_form_error(SharedString::default());
    ui.set_card_modal_loading(false);
    clear_card_errors(ui);
}

fn clear_card_errors(ui: &MainWindow) {
    ui.set_card_error_number(SharedString::default());
    ui.set_card_error_holder(SharedString::default());
    ui.set_card_error_expiry(SharedString::default());
    ui.set_card_error_cvv(SharedString::default());
    ui.set_card_error_cpf(SharedString::default());
    ui.set_card_error_birth(SharedString::default());
    ui.set_card_error_email(SharedString::default());
    ui.set_card_error_phone(SharedString::default());
}

/// Erros de validação por campo do formulário de cartão. Campo vazio
/// = sem erro. Mensagens em pt-BR exibidas abaixo de cada campo.
#[derive(Default)]
struct CardFieldErrors {
    number: String,
    holder: String,
    expiry: String,
    cvv: String,
    cpf: String,
    birth: String,
    email: String,
    phone: String,
}

impl CardFieldErrors {
    fn is_empty(&self) -> bool {
        self.number.is_empty()
            && self.holder.is_empty()
            && self.expiry.is_empty()
            && self.cvv.is_empty()
            && self.cpf.is_empty()
            && self.birth.is_empty()
            && self.email.is_empty()
            && self.phone.is_empty()
    }
}

fn apply_card_errors(ui: &MainWindow, e: &CardFieldErrors) {
    ui.set_card_error_number(SharedString::from(&e.number));
    ui.set_card_error_holder(SharedString::from(&e.holder));
    ui.set_card_error_expiry(SharedString::from(&e.expiry));
    ui.set_card_error_cvv(SharedString::from(&e.cvv));
    ui.set_card_error_cpf(SharedString::from(&e.cpf));
    ui.set_card_error_birth(SharedString::from(&e.birth));
    ui.set_card_error_email(SharedString::from(&e.email));
    ui.set_card_error_phone(SharedString::from(&e.phone));
}

/// Só dígitos de uma string mascarada.
fn digits(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_digit()).collect()
}

/// Valida o formulário e devolve o corpo já com valores limpos
/// (CPF/telefone só dígitos, nascimento em "AAAA-MM-DD"). Em caso de
/// erro, devolve as mensagens por campo.
//
// `CardFieldErrors` é um bag de 8 mensagens por campo; retorná-lo por
// valor no `Err` é aceitável neste caminho frio (submit do formulário).
// Boxear só espalharia `Box` aos callers sem ganho real (AI_RULES §13).
#[allow(clippy::result_large_err)]
fn validate_card_form(ui: &MainWindow) -> Result<CardSubscribeBody, CardFieldErrors> {
    let mut e = CardFieldErrors::default();

    let number = digits(ui.get_card_form_number().as_ref());
    if number.is_empty() {
        e.number = "Informe o número do cartão".into();
    } else if !(13..=19).contains(&number.len()) {
        e.number = "Número do cartão inválido".into();
    }

    let holder = ui.get_card_form_holder().to_string().trim().to_string();
    if holder.is_empty() {
        e.holder = "Informe o nome impresso no cartão".into();
    }

    // Validade: "MM/AA".
    let exp_digits = digits(ui.get_card_form_expiry().as_ref());
    let expiry = if exp_digits.len() != 4 {
        e.expiry = "Validade incompleta (MM/AA)".into();
        String::new()
    } else {
        let mm: u8 = exp_digits[..2].parse().unwrap_or(0);
        if !(1..=12).contains(&mm) {
            e.expiry = "Mês inválido (01–12)".into();
            String::new()
        } else {
            format!("{}/{}", &exp_digits[..2], &exp_digits[2..])
        }
    };

    let cvv = digits(ui.get_card_form_cvv().as_ref());
    if !(3..=4).contains(&cvv.len()) {
        e.cvv = "CVV inválido".into();
    }

    let cpf = digits(ui.get_card_form_cpf().as_ref());
    if cpf.len() != 11 && cpf.len() != 14 {
        e.cpf = "CPF (11) ou CNPJ (14) inválido".into();
    }

    // Nascimento: UI em "DD/MM/AAAA" → envia "AAAA-MM-DD".
    let birth_digits = digits(ui.get_card_form_birth().as_ref());
    let birth_iso = if birth_digits.len() != 8 {
        e.birth = "Data incompleta (DD/MM/AAAA)".into();
        String::new()
    } else {
        let iso = format!(
            "{}-{}-{}",
            &birth_digits[4..8],
            &birth_digits[2..4],
            &birth_digits[..2]
        );
        if chrono::NaiveDate::parse_from_str(&iso, "%Y-%m-%d").is_err() {
            e.birth = "Data inválida".into();
            String::new()
        } else {
            iso
        }
    };

    let email = ui.get_card_form_email().to_string().trim().to_string();
    let email_ok = email.contains('@')
        && email.rsplit('@').next().map(|d| d.contains('.')).unwrap_or(false);
    if !email_ok {
        e.email = "E-mail inválido".into();
    }

    let phone = digits(ui.get_card_form_phone().as_ref());
    if phone.len() != 10 && phone.len() != 11 {
        e.phone = "Telefone inválido com DDD".into();
    }

    if !e.is_empty() {
        return Err(e);
    }
    Ok(CardSubscribeBody {
        number,
        holder_name: holder,
        expiry,
        cvv,
        brand: String::new(),
        cpf,
        email,
        phone,
        birth: birth_iso,
    })
}

fn set_card_error(ui_weak: &slint::Weak<MainWindow>, message: String) {
    let ui_weak = ui_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_card_modal_loading(false);
            ui.set_card_form_error(SharedString::from(message));
        }
    });
}

/// Toast simples a partir de um `Weak` (fora do event loop).
pub(crate) fn toast(ui_weak: &slint::Weak<MainWindow>, message: String, tone: &'static str) {
    let ui_weak = ui_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_weak.upgrade() {
            show_toast(&ui, &message, tone);
        }
    });
}

pub(crate) fn refresh(ui_weak: &slint::Weak<MainWindow>) {
    let ui_weak = ui_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.invoke_subscription_refresh();
        }
    });
}

