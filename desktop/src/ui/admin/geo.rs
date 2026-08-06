//! Consultas geográficas de conveniência do cadastro: CEP (ViaCEP) e
//! geocodificação do endereço (OpenStreetMap/Nominatim).
//!
//! Regras (§1/§3/§11): a busca fica no Rust, não na UI, e é só UX — o
//! backend não depende dela (apenas armazena o que for enviado).

use slint::{ComponentHandle, SharedString};

use crate::{AdminState, MainWindow, HTTP_CLIENT};

/// Consulta o CEP (ViaCEP) e preenche cidade/UF. É conveniência de UX: o
/// operador ainda pode editar; o backend não depende disto.
/// Falha silenciosa (rede/CEP inexistente) — o operador digita manualmente.
pub(super) fn setup_company_cep(ui: &MainWindow, handle: &tokio::runtime::Handle) {
    let ui_weak = ui.as_weak();
    let handle = handle.clone();
    ui.global::<AdminState>().on_company_cep_changed(move |raw| {
        // Só dispara com 8 dígitos (CEP completo).
        let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
        if digits.len() != 8 {
            return;
        }
        let ui_weak = ui_weak.clone();
        handle.spawn(async move {
            let uw = ui_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = uw.upgrade() {
                    ui.global::<AdminState>().set_company_cep_loading(true);
                }
            });
            let res = fetch_via_cep(&digits).await;
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                ui.global::<AdminState>().set_company_cep_loading(false);
                if let Some(v) = res {
                    apply_via_cep(&ui, &v);
                }
            });
        });
    });
}

#[derive(serde::Deserialize)]
struct ViaCep {
    #[serde(default)] localidade: String,
    #[serde(default)] uf: String,
    #[serde(default)] erro: bool,
}

/// Busca o CEP no ViaCEP (`None` em qualquer falha de rede/resposta).
async fn fetch_via_cep(digits: &str) -> Option<ViaCep> {
    match HTTP_CLIENT
        .get(format!("https://viacep.com.br/ws/{digits}/json/"))
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r.json::<ViaCep>().await.ok(),
        _ => None,
    }
}

/// Preenche cidade/UF com o que veio do ViaCEP (campos vazios são ignorados).
fn apply_via_cep(ui: &MainWindow, v: &ViaCep) {
    if v.erro {
        return;
    }
    if !v.localidade.is_empty() {
        ui.global::<AdminState>().set_company_form_city(SharedString::from(v.localidade.as_str()));
    }
    if !v.uf.is_empty() {
        ui.global::<AdminState>().set_company_form_uf(SharedString::from(v.uf.as_str()));
    }
}

