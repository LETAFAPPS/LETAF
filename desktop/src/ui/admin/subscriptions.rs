//! Assinatura de uma empresa e seu histórico de faturas.

use std::sync::Arc;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use tokio::sync::RwLock;

use crate::{AdminInvoiceRow, AdminState, MainWindow, HTTP_CLIENT};

use super::super::helpers::show_toast;
use super::dto::InvoiceDto;
use super::http::{get_json, report_modal, write_outcome};

/// Registra os callbacks de assinatura/faturas.
pub(super) fn setup_subscriptions(
    ui: &MainWindow,
    handle: &tokio::runtime::Handle,
    auth_token: &Arc<RwLock<Option<String>>>,
    server_url: &str,
) {
    setup_load_invoices(ui, handle, auth_token, server_url);
    setup_mark_invoice_paid(ui, handle, auth_token, server_url);
    setup_save_subscription(ui, handle, auth_token, server_url);
}

/// Faturas: carregar o histórico da empresa em edição.
fn setup_load_invoices(
    ui: &MainWindow,
    handle: &tokio::runtime::Handle,
    auth_token: &Arc<RwLock<Option<String>>>,
    server_url: &str,
) {
    let ui_weak = ui.as_weak();
    let handle = handle.clone();
    let auth_token = auth_token.clone();
    let server_url = server_url.to_string();
    ui.global::<AdminState>().on_load_invoices(move |company_id| {
        let company_id = company_id.to_string();
        if company_id.is_empty() {
            return;
        }
        let ui_weak = ui_weak.clone();
        let auth_token = auth_token.clone();
        let server_url = server_url.clone();
        handle.spawn(async move {
            let Some(token) = auth_token.read().await.clone() else { return };
            let invoices: Vec<InvoiceDto> = get_json(
                &format!("{server_url}/admin/companies/{company_id}/invoices"),
                &token,
            )
            .await
            .unwrap_or_default();
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                let rows: Vec<AdminInvoiceRow> = invoices.into_iter().map(invoice_row).collect();
                ui.global::<AdminState>().set_invoices(ModelRc::new(VecModel::from(rows)));
            });
        });
    });
}

fn invoice_row(i: InvoiceDto) -> AdminInvoiceRow {
    AdminInvoiceRow {
        id: i.id.into(),
        number: i.number.into(),
        description: i.description.into(),
        amount: i.amount.into(),
        status: i.status.into(),
        issued_at: i.issued_at.into(),
        paid_at: i.paid_at.into(),
        method: i.method.into(),
    }
}

/// Faturas: baixa manual de uma fatura pendente.
fn setup_mark_invoice_paid(
    ui: &MainWindow,
    handle: &tokio::runtime::Handle,
    auth_token: &Arc<RwLock<Option<String>>>,
    server_url: &str,
) {
    let ui_weak = ui.as_weak();
    let handle = handle.clone();
    let auth_token = auth_token.clone();
    let server_url = server_url.to_string();
    ui.global::<AdminState>().on_mark_invoice_paid(move |invoice_id| {
        let Some(ui) = ui_weak.upgrade() else { return };
        let company_id = ui.global::<AdminState>().get_sub_edit_company_id().to_string();
        let invoice_id = invoice_id.to_string();
        if company_id.is_empty() || invoice_id.is_empty() {
            return;
        }
        let ui_weak = ui.as_weak();
        let auth_token = auth_token.clone();
        let server_url = server_url.clone();
        handle.spawn(async move {
            let Some(token) = auth_token.read().await.clone() else { return };
            let result = HTTP_CLIENT
                .put(format!(
                    "{server_url}/admin/companies/{company_id}/invoices/{invoice_id}/paid"
                ))
                .bearer_auth(&token)
                .send()
                .await;
            report_modal(ui_weak, result, "Fatura marcada como paga", move |ui| {
                // Recarrega faturas e listas (o status da assinatura pode
                // ter voltado a "ativa").
                ui.global::<AdminState>()
                    .invoke_load_invoices(SharedString::from(company_id.as_str()));
            })
            .await;
        });
    });
}

/// Gestão da assinatura de uma empresa (plano, status e desconto).
fn setup_save_subscription(
    ui: &MainWindow,
    handle: &tokio::runtime::Handle,
    auth_token: &Arc<RwLock<Option<String>>>,
    server_url: &str,
) {
    let ui_weak = ui.as_weak();
    let handle = handle.clone();
    let auth_token = auth_token.clone();
    let server_url = server_url.to_string();
    ui.global::<AdminState>().on_save_subscription(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        let company_id = ui.global::<AdminState>().get_sub_edit_company_id().to_string();
        if company_id.is_empty() {
            return;
        }
        let body = subscription_body(&ui);
        ui.global::<AdminState>().set_sub_edit_busy(true);
        let ui_weak = ui.as_weak();
        let auth_token = auth_token.clone();
        let server_url = server_url.clone();
        handle.spawn(async move {
            let Some(token) = auth_token.read().await.clone() else { return };
            let result = HTTP_CLIENT
                .put(format!("{server_url}/admin/subscriptions/{company_id}"))
                .bearer_auth(&token)
                .json(&body)
                .send()
                .await;
            let outcome = write_outcome(result).await;
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                ui.global::<AdminState>().set_sub_edit_busy(false);
                match outcome {
                    Ok(()) => {
                        show_toast(&ui, "Assinatura Atualizada", "success");
                        ui.global::<AdminState>().set_sub_edit_open(false);
                        ui.global::<AdminState>().invoke_refresh();
                    }
                    Err(msg) => show_toast(&ui, &msg, "error"),
                }
            });
        });
    });
}

/// Corpo do PUT da assinatura (plano, status e desconto do formulário).
fn subscription_body(ui: &MainWindow) -> serde_json::Value {
    let g = ui.global::<AdminState>();
    let plan = g.get_sub_edit_plan().to_string();
    let status = g.get_sub_edit_status().to_string();
    let discount_name = g.get_sub_edit_discount_name().to_string();
    // Aceita vírgula ou ponto como separador decimal.
    let discount: f64 = g
        .get_sub_edit_discount()
        .replace('.', "")
        .replace(',', ".")
        .trim()
        .parse()
        .unwrap_or(0.0);
    serde_json::json!({
        "plan": plan, "status": status, "discount": discount,
        "discount_name": discount_name,
    })
}
