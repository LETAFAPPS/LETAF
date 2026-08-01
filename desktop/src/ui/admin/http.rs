//! Chamadas HTTP autenticadas do painel e o feedback padrão das escritas.
//!
//! O painel é ONLINE (não há espelho local do cross-tenant): toda leitura
//! e escrita vai à API `/admin/*` com o JWT do super admin.

use serde::de::DeserializeOwned;
use slint::{ComponentHandle, SharedString};

use crate::{AdminState, MainWindow, HTTP_CLIENT};

use super::super::helpers::show_toast;

/// GET autenticado → desserializa em `T`. `None` em qualquer falha.
pub(super) async fn get_json<T: DeserializeOwned>(url: &str, token: &str) -> Option<T> {
    match HTTP_CLIENT.get(url).bearer_auth(token).send().await {
        Ok(resp) if resp.status().is_success() => resp.json::<T>().await.ok(),
        Ok(resp) => {
            tracing::warn!("GET {url} → {}", resp.status());
            None
        }
        Err(e) => {
            tracing::info!("GET {url} falhou: {e}");
            None
        }
    }
}

/// Converte a resposta HTTP num `Result<(), String>`, extraindo a mensagem
/// de erro do corpo quando o servidor rejeita (4xx). Fonte única do parser
/// de erro para os `report*` (§8).
pub(super) async fn write_outcome(
    result: Result<reqwest::Response, reqwest::Error>,
) -> Result<(), String> {
    match result {
        Ok(resp) if resp.status().is_success() => Ok(()),
        Ok(resp) => {
            let body = resp.text().await.unwrap_or_default();
            let msg = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Não foi possível concluir a operação".into());
            Err(msg)
        }
        Err(_) => Err("Sem conexão com o servidor".into()),
    }
}

/// Feedback padrão de uma escrita: toast de sucesso, o fechamento/limpeza
/// específico da tela (`on_success`) e recarga das listas. Em erro, só o
/// toast — a tela continua como está, com os dados digitados.
pub(super) async fn report_modal(
    ui_weak: slint::Weak<MainWindow>,
    result: Result<reqwest::Response, reqwest::Error>,
    ok_msg: &'static str,
    on_success: impl FnOnce(&MainWindow) + Send + 'static,
) {
    let outcome = write_outcome(result).await;
    let _ = slint::invoke_from_event_loop(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        match outcome {
            Ok(()) => {
                show_toast(&ui, ok_msg, "success");
                on_success(&ui);
                ui.global::<AdminState>().invoke_refresh();
            }
            Err(msg) => show_toast(&ui, &msg, "error"),
        }
    });
}

/// Feedback + refresh após salvar/excluir um administrador.
pub(super) async fn report(
    ui_weak: slint::Weak<MainWindow>,
    result: Result<reqwest::Response, reqwest::Error>,
    ok_msg: &'static str,
) {
    report_modal(ui_weak, result, ok_msg, |ui| {
        // Limpa o formulário e recarrega as listas.
        let g = ui.global::<AdminState>();
        g.set_form_id(SharedString::new());
        g.set_form_name(SharedString::new());
        g.set_form_email(SharedString::new());
        g.set_form_password(SharedString::new());
    })
    .await;
}
