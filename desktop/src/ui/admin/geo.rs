//! Consultas geográficas de conveniência do cadastro: CEP (ViaCEP) e
//! geocodificação do endereço (OpenStreetMap/Nominatim).
//!
//! Regras (§1/§3/§11): a busca fica no Rust, não na UI, e é só UX — o
//! backend não depende dela (apenas armazena o que for enviado).

use slint::{ComponentHandle, SharedString};

use crate::{AdminState, MainWindow, HTTP_CLIENT};

use super::super::helpers::show_toast;

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

/// Geocodifica o endereço preenchido → latitude/longitude, via
/// OpenStreetMap/Nominatim (gratuito, sem chave; exige User-Agent). É só
/// conveniência de UX: o backend apenas armazena as coordenadas (§11).
pub(super) fn setup_company_geocode(ui: &MainWindow, handle: &tokio::runtime::Handle) {
    let ui_weak = ui.as_weak();
    let handle = handle.clone();
    ui.global::<AdminState>().on_company_geocode(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        let query = address_query(&ui);
        if query.is_empty() {
            show_toast(&ui, "Preencha o endereço antes de buscar as coordenadas", "error");
            return;
        }
        let query = format!("{query}, Brasil");
        let ui_weak = ui_weak.clone();
        handle.spawn(async move {
            let uw = ui_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = uw.upgrade() {
                    ui.global::<AdminState>().set_company_geocode_loading(true);
                }
            });
            let hit = geocode(&query).await;
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                ui.global::<AdminState>().set_company_geocode_loading(false);
                match hit {
                    Some(h) if !h.lat.is_empty() && !h.lon.is_empty() => {
                        ui.global::<AdminState>().set_company_form_latitude(h.lat.into());
                        ui.global::<AdminState>().set_company_form_longitude(h.lon.into());
                        show_toast(&ui, "Coordenadas encontradas", "success");
                    }
                    _ => show_toast(&ui, "Não foi possível localizar o endereço", "error"),
                }
            });
        });
    });
}

/// Monta o endereço a partir dos campos já preenchidos.
fn address_query(ui: &MainWindow) -> String {
    let g = ui.global::<AdminState>();
    let parts = [
        g.get_company_form_address().trim().to_string(),
        g.get_company_form_neighborhood().trim().to_string(),
        g.get_company_form_city().trim().to_string(),
        g.get_company_form_uf().trim().to_string(),
    ];
    parts.iter().filter(|p| !p.is_empty()).cloned().collect::<Vec<_>>().join(", ")
}

#[derive(serde::Deserialize)]
struct Hit {
    #[serde(default)] lat: String,
    #[serde(default)] lon: String,
}

/// Primeiro resultado do Nominatim para o endereço (`None` em falha).
async fn geocode(query: &str) -> Option<Hit> {
    let hits: Option<Vec<Hit>> = match HTTP_CLIENT
        .get("https://nominatim.openstreetmap.org/search")
        .query(&[("q", query), ("format", "json"), ("limit", "1")])
        // Nominatim exige identificar o app no User-Agent.
        .header("User-Agent", "LETAF-ERP/1.0 (suporte@letaf.app)")
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r.json::<Vec<Hit>>().await.ok(),
        _ => None,
    };
    hits.and_then(|v| v.into_iter().next())
}
