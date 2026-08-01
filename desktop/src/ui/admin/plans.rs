//! Catálogo de planos: formulário (novo/editar/busca) e persistência.

use std::sync::Arc;

use slint::{ComponentHandle, SharedString};
use tokio::sync::RwLock;

use crate::{AdminState, MainWindow, HTTP_CLIENT};

use super::super::helpers::show_toast;
use super::cache::PlansCache;
use super::filters::apply_plan_filter;
use super::http::{report, report_modal};

/// Formulário de plano: novo (limpa) e editar (preenche do cache).
pub(super) fn setup_plan_form(ui: &MainWindow, plans_cache: &PlansCache) {
    setup_plan_new(ui);
    setup_plan_edit(ui, plans_cache);
    setup_plan_search(ui, plans_cache);
}

/// "+": abre o modal de cadastro limpo.
fn setup_plan_new(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.global::<AdminState>().on_plan_new(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        ui.global::<AdminState>().set_plan_id(SharedString::new());
        ui.global::<AdminState>().set_plan_name(SharedString::new());
        ui.global::<AdminState>().set_plan_amount(SharedString::new());
        ui.global::<AdminState>().set_plan_period(SharedString::new());
        ui.global::<AdminState>().set_plan_trial(SharedString::new());
        ui.global::<AdminState>().set_plan_description(SharedString::new());
        ui.global::<AdminState>().set_plan_highlight(SharedString::new());
        ui.global::<AdminState>().set_plan_active(true);
        ui.global::<AdminState>().set_plan_modal_open(true);
    });
}

/// Ícone de editar: abre o modal pré-preenchido com o plano do cache.
fn setup_plan_edit(ui: &MainWindow, plans_cache: &PlansCache) {
    let ui_weak = ui.as_weak();
    let plans_cache = plans_cache.clone();
    ui.global::<AdminState>().on_plan_edit(move |id| {
        let Some(ui) = ui_weak.upgrade() else { return };
        let Ok(g) = plans_cache.lock() else { return };
        if let Some(p) = g.iter().find(|p| p.id == id.as_str()) {
            ui.global::<AdminState>().set_plan_id(p.id.clone().into());
            ui.global::<AdminState>().set_plan_name(p.name.clone().into());
            // Valores numéricos com vírgula (padrão pt-BR).
            ui.global::<AdminState>().set_plan_amount(format!("{:.2}", p.amount).replace('.', ",").into());
            ui.global::<AdminState>().set_plan_period(p.period_days.to_string().into());
            ui.global::<AdminState>().set_plan_trial(p.trial_days.to_string().into());
            ui.global::<AdminState>().set_plan_description(p.description.clone().into());
            ui.global::<AdminState>().set_plan_highlight(p.highlight_label.clone().into());
            ui.global::<AdminState>().set_plan_active(p.active);
            ui.global::<AdminState>().set_plan_modal_open(true);
        }
    });
}

/// Busca por nome (reaplica sobre o cache).
fn setup_plan_search(ui: &MainWindow, plans_cache: &PlansCache) {
    let ui_weak = ui.as_weak();
    let plans_cache = plans_cache.clone();
    ui.global::<AdminState>().on_filter_plans(move || {
        if let Some(ui) = ui_weak.upgrade() {
            apply_plan_filter(&ui, &plans_cache);
        }
    });
}

/// Salvar (criar/atualizar) e excluir plano.
pub(super) fn setup_plan_persist(
    ui: &MainWindow,
    handle: &tokio::runtime::Handle,
    auth_token: &Arc<RwLock<Option<String>>>,
    server_url: &str,
) {
    setup_plan_save(ui, handle, auth_token, server_url);
    setup_plan_delete(ui, handle, auth_token, server_url);
}

/// Campos numéricos/textuais do formulário de plano.
struct PlanForm {
    id: String,
    name: String,
    amount: f64,
    period: i32,
    body: serde_json::Value,
}

/// Salvar.
fn setup_plan_save(
    ui: &MainWindow,
    handle: &tokio::runtime::Handle,
    auth_token: &Arc<RwLock<Option<String>>>,
    server_url: &str,
) {
    let ui_weak = ui.as_weak();
    let handle = handle.clone();
    let auth_token = auth_token.clone();
    let server_url = server_url.to_string();
    ui.global::<AdminState>().on_plan_save(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        let form = read_plan_form(&ui);
        if form.name.is_empty() {
            show_toast(&ui, "Informe o nome do plano", "error");
            return;
        }
        if form.amount <= 0.0 || form.period < 1 {
            show_toast(&ui, "Valor e período devem ser válidos", "error");
            return;
        }
        let PlanForm { id, body, .. } = form;
        let ui_weak = ui.as_weak();
        let auth_token = auth_token.clone();
        let server_url = server_url.clone();
        handle.spawn(async move {
            let Some(token) = auth_token.read().await.clone() else { return };
            let result = if id.is_empty() {
                HTTP_CLIENT
                    .post(format!("{server_url}/admin/plans"))
                    .bearer_auth(&token)
                    .json(&body)
                    .send()
                    .await
            } else {
                HTTP_CLIENT
                    .put(format!("{server_url}/admin/plans/{id}"))
                    .bearer_auth(&token)
                    .json(&body)
                    .send()
                    .await
            };
            report_modal(ui_weak, result, "Plano Salvo", |ui| {
                ui.global::<AdminState>().set_plan_modal_open(false);
            })
            .await;
        });
    });
}

/// Lê o formulário de plano (já com o corpo JSON montado).
fn read_plan_form(ui: &MainWindow) -> PlanForm {
    let g = ui.global::<AdminState>();
    let id = g.get_plan_id().to_string();
    let name = g.get_plan_name().trim().to_string();
    // Aceita vírgula ou ponto como separador decimal.
    let amount: f64 = g
        .get_plan_amount()
        .replace('.', "")
        .replace(',', ".")
        .trim()
        .parse()
        .unwrap_or(0.0);
    let period: i32 = g.get_plan_period().trim().parse().unwrap_or(0);
    let trial: i32 = g.get_plan_trial().trim().parse().unwrap_or(0);
    let description = g.get_plan_description().to_string();
    let highlight = g.get_plan_highlight().to_string();
    let active = g.get_plan_active();
    let body = serde_json::json!({
        "name": name, "amount": amount, "period_days": period,
        "trial_days": trial, "description": description,
        "highlight_label": highlight, "active": active,
    });
    PlanForm { id, name, amount, period, body }
}

/// Excluir.
fn setup_plan_delete(
    ui: &MainWindow,
    handle: &tokio::runtime::Handle,
    auth_token: &Arc<RwLock<Option<String>>>,
    server_url: &str,
) {
    let ui_weak = ui.as_weak();
    let handle = handle.clone();
    let auth_token = auth_token.clone();
    let server_url = server_url.to_string();
    ui.global::<AdminState>().on_plan_delete(move |id| {
        let id = id.to_string();
        let ui_weak = ui_weak.clone();
        let auth_token = auth_token.clone();
        let server_url = server_url.clone();
        handle.spawn(async move {
            let Some(token) = auth_token.read().await.clone() else { return };
            let result = HTTP_CLIENT
                .delete(format!("{server_url}/admin/plans/{id}"))
                .bearer_auth(&token)
                .send()
                .await;
            report(ui_weak, result, "Plano Removido").await;
        });
    });
}
