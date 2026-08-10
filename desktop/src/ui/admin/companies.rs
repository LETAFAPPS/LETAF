//! Empresas (tenants): cadastro/edição, suspensão, exclusão e o detalhe
//! consolidado usado no suporte.

use std::sync::Arc;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use tokio::sync::RwLock;

use crate::{
    AdminCompanyDetail, AdminCompanyOrderRow, AdminState, MainWindow, HTTP_CLIENT,
};

use super::super::helpers::show_toast;
use super::company_form::{clear_company_form, fill_company_form};
use super::dto::{CompanyDetailDto, CompanyFormDto, CompanyOrderDto};
use super::format::parse_money_br;
use super::http::{get_json, report, report_modal};

/// Registra os callbacks das Empresas.
pub(super) fn setup_companies(
    ui: &MainWindow,
    handle: &tokio::runtime::Handle,
    auth_token: &Arc<RwLock<Option<String>>>,
    server_url: &str,
) {
    setup_save_company(ui, handle, auth_token, server_url);
    setup_new_form(ui);
    setup_company_edit(ui, handle, auth_token, server_url);
    setup_company_active(ui, handle, auth_token, server_url);
    setup_company_delete(ui, handle, auth_token, server_url);
    setup_company_detail(ui, handle, auth_token, server_url);
}

/// Campos que decidem o modo de envio e a validação do cadastro.
struct CompanyFormHead {
    name: String,
    subdomain: String,
    editing: bool,
    edit_id: String,
    admin_name: String,
    admin_email: String,
    admin_password: String,
}

/// Cadastro de estabelecimento (empresa + admin inicial + infos) via
/// POST /admin/companies. O form é grande → lê as propriedades da UI
/// (sem callback com dezenas de args).
fn setup_save_company(
    ui: &MainWindow,
    handle: &tokio::runtime::Handle,
    auth_token: &Arc<RwLock<Option<String>>>,
    server_url: &str,
) {
    let ui_weak = ui.as_weak();
    let handle = handle.clone();
    let auth_token = auth_token.clone();
    let server_url = server_url.to_string();
    ui.global::<AdminState>().on_save_company(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        let head = read_form_head(&ui);
        if !company_form_valid(&ui, &head) {
            return;
        }
        let body = build_company_body(&ui, &head);
        let (editing, edit_id) = (head.editing, head.edit_id);
        let ui_weak = ui.as_weak();
        let auth_token = auth_token.clone();
        let server_url = server_url.clone();
        handle.spawn(async move {
            let Some(token) = auth_token.read().await.clone() else { return };
            // Edição → PUT no id; criação → POST. Campos extras do body
            // (admin_*) são ignorados pelo handler de update.
            let req = if editing {
                HTTP_CLIENT.put(format!("{server_url}/admin/companies/{edit_id}"))
            } else {
                HTTP_CLIENT.post(format!("{server_url}/admin/companies"))
            };
            let result = req.bearer_auth(&token).json(&body).send().await;
            let ok_msg = if editing { "Alterações Salvas" } else { "Estabelecimento Cadastrado" };
            report_modal(ui_weak, result, ok_msg, |ui| {
                clear_company_form(ui);
                // Volta à lista e limpa os erros/modo edição (senão o
                // formulário fica todo vermelho com os campos zerados).
                let g = ui.global::<AdminState>();
                g.set_company_editing(false);
                g.set_company_edit_id(SharedString::new());
                g.set_company_form_attempted(false);
                g.set_company_show_form(false);
            })
            .await;
        });
    });
}

fn read_form_head(ui: &MainWindow) -> CompanyFormHead {
    let g = ui.global::<AdminState>();
    CompanyFormHead {
        name: g.get_company_form_name().trim().to_string(),
        subdomain: g.get_company_form_subdomain().trim().to_lowercase(),
        editing: g.get_company_editing(),
        edit_id: g.get_company_edit_id().to_string(),
        admin_name: g.get_company_form_admin_name().trim().to_string(),
        admin_email: g.get_company_form_admin_email().trim().to_string(),
        admin_password: g.get_company_form_admin_password().to_string(),
    }
}

/// `true` se o cadastro pode ser enviado; senão mostra o toast do erro.
/// É só UX — a validação que vale é a do backend (§11).
fn company_form_valid(ui: &MainWindow, h: &CompanyFormHead) -> bool {
    if h.editing {
        // Na edição não mexemos no admin inicial nem no subdomínio.
        if h.name.is_empty() {
            show_toast(ui, "Informe o nome da empresa", "error");
            return false;
        }
        return true;
    }
    if h.name.is_empty() || h.subdomain.is_empty() || h.admin_name.is_empty() || h.admin_email.is_empty() {
        show_toast(ui, "Preencha empresa, subdomínio, nome e e-mail do admin", "error");
        return false;
    }
    if h.admin_password.trim().is_empty() {
        show_toast(ui, "Defina uma senha para o administrador", "error");
        return false;
    }
    true
}

/// Corpo JSON do cadastro, lido direto das propriedades do formulário.
fn build_company_body(ui: &MainWindow, h: &CompanyFormHead) -> serde_json::Value {
    let g = ui.global::<AdminState>();
    let discount = parse_money_br(&g.get_company_form_discount());
    serde_json::json!({
        "name": h.name,
        "subdomain": h.subdomain,
        "admin_name": h.admin_name,
        "admin_email": h.admin_email,
        "admin_password": h.admin_password,
        "admin_phone": g.get_company_form_admin_phone().trim(),
        "phone": g.get_company_form_phone().trim(),
        "whatsapp": g.get_company_form_whatsapp().trim(),
        "email": g.get_company_form_email().trim(),
        "document": g.get_company_form_document().trim(),
        "address": g.get_company_form_address().trim(),
        "neighborhood": g.get_company_form_neighborhood().trim(),
        "zip_code": g.get_company_form_zip().trim(),
        "city": g.get_company_form_city().trim(),
        "uf": g.get_company_form_uf().trim(),
        "location_url": g.get_company_form_location_url().trim(),
        "plan": g.get_company_form_plan().to_string(),
        "business_type": g.get_company_form_business_type().to_string(),
        "trial_days": g.get_company_form_trial().trim().parse::<i32>().unwrap_or(0),
        "logo_data": g.get_company_form_logo_data().to_string(),
        "cover_data": g.get_company_form_cover_data().to_string(),
        "plan_discount": discount,
    })
}

/// "+": abre um cadastro LIMPO (sem sobras de uma edição anterior nem
/// erros de uma tentativa passada).
fn setup_new_form(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.global::<AdminState>().on_company_new_form(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        clear_company_form(&ui);
        ui.global::<AdminState>().set_company_editing(false);
        ui.global::<AdminState>().set_company_edit_id(SharedString::new());
        ui.global::<AdminState>().set_company_form_attempted(false);
        ui.global::<AdminState>().set_company_show_form(true);
    });
}

/// Ícone de editar: carrega os dados da empresa (GET .../form) e abre o
/// cadastro em modo edição (PUT no save).
fn setup_company_edit(
    ui: &MainWindow,
    handle: &tokio::runtime::Handle,
    auth_token: &Arc<RwLock<Option<String>>>,
    server_url: &str,
) {
    let ui_weak = ui.as_weak();
    let handle = handle.clone();
    let auth_token = auth_token.clone();
    let server_url = server_url.to_string();
    ui.global::<AdminState>().on_company_edit(move |id| {
        let id = id.to_string();
        let ui_weak = ui_weak.clone();
        let auth_token = auth_token.clone();
        let server_url = server_url.clone();
        handle.spawn(async move {
            let Some(token) = auth_token.read().await.clone() else { return };
            let resp = HTTP_CLIENT
                .get(format!("{server_url}/admin/companies/{id}/form"))
                .bearer_auth(&token)
                .send()
                .await;
            let form: Option<CompanyFormDto> = match resp {
                Ok(r) if r.status().is_success() => r.json().await.ok(),
                _ => None,
            };
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                match form {
                    Some(f) => fill_company_form(&ui, &f),
                    None => show_toast(&ui, "Não foi possível carregar a empresa", "error"),
                }
            });
        });
    });
}

/// Suspender/reativar acesso do tenant (super admin).
fn setup_company_active(
    ui: &MainWindow,
    handle: &tokio::runtime::Handle,
    auth_token: &Arc<RwLock<Option<String>>>,
    server_url: &str,
) {
    let ui_weak = ui.as_weak();
    let handle = handle.clone();
    let auth_token = auth_token.clone();
    let server_url = server_url.to_string();
    ui.global::<AdminState>().on_set_company_active(move |id, active| {
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
                .put(format!("{server_url}/admin/companies/{id}/active"))
                .bearer_auth(&token)
                .json(&serde_json::json!({ "active": active }))
                .send()
                .await;
            let msg = if active { "Empresa Reativada" } else { "Empresa Suspensa" };
            report(ui_weak, result, msg).await;
        });
    });
}

/// Excluir empresa (soft delete via API admin, cross-tenant).
fn setup_company_delete(
    ui: &MainWindow,
    handle: &tokio::runtime::Handle,
    auth_token: &Arc<RwLock<Option<String>>>,
    server_url: &str,
) {
    let ui_weak = ui.as_weak();
    let handle = handle.clone();
    let auth_token = auth_token.clone();
    let server_url = server_url.to_string();
    ui.global::<AdminState>().on_confirm_delete_company(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        let id = ui.global::<AdminState>().get_del_company_id().to_string();
        ui.global::<AdminState>().set_del_company_open(false);
        if id.is_empty() {
            return;
        }
        let ui_weak = ui.as_weak();
        let auth_token = auth_token.clone();
        let server_url = server_url.clone();
        handle.spawn(async move {
            let Some(token) = auth_token.read().await.clone() else { return };
            let result = HTTP_CLIENT
                .delete(format!("{server_url}/admin/companies/{id}"))
                .bearer_auth(&token)
                .send()
                .await;
            report_modal(ui_weak, result, "Empresa Excluída", |ui| {
                // Se a exclusão veio da tela de edição, volta à lista.
                clear_company_form(ui);
                let g = ui.global::<AdminState>();
                g.set_company_editing(false);
                g.set_company_edit_id(SharedString::new());
                g.set_company_show_form(false);
            })
            .await;
        });
    });
}

/// Detalhe consolidado de uma empresa (modal de suporte).
fn setup_company_detail(
    ui: &MainWindow,
    handle: &tokio::runtime::Handle,
    auth_token: &Arc<RwLock<Option<String>>>,
    server_url: &str,
) {
    let ui_weak = ui.as_weak();
    let handle = handle.clone();
    let auth_token = auth_token.clone();
    let server_url = server_url.to_string();
    ui.global::<AdminState>().on_open_company_detail(move |id| {
        let id = id.to_string();
        if id.is_empty() {
            return;
        }
        let ui_weak = ui_weak.clone();
        let auth_token = auth_token.clone();
        let server_url = server_url.clone();
        handle.spawn(async move {
            let Some(token) = auth_token.read().await.clone() else { return };
            let Some(d): Option<CompanyDetailDto> =
                get_json(&format!("{server_url}/admin/companies/{id}"), &token).await
            else {
                return;
            };
            let orders: Vec<CompanyOrderDto> =
                get_json(&format!("{server_url}/admin/companies/{id}/orders"), &token)
                    .await
                    .unwrap_or_default();
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                apply_company_detail(&ui, d, orders);
            });
        });
    });
}

/// Reflete o detalhe + os últimos pedidos no modal e o abre.
fn apply_company_detail(ui: &MainWindow, d: CompanyDetailDto, orders: Vec<CompanyOrderDto>) {
    ui.global::<AdminState>().set_detail(company_detail(d));
    let order_rows: Vec<AdminCompanyOrderRow> = orders
        .into_iter()
        .map(|o| AdminCompanyOrderRow {
            number: o.number as i32,
            status: o.status.into(),
            total: o.total.into(),
            at: o.at.into(),
        })
        .collect();
    ui.global::<AdminState>().set_detail_orders(ModelRc::new(VecModel::from(order_rows)));
    ui.global::<AdminState>().set_detail_open(true);
}

fn company_detail(d: CompanyDetailDto) -> AdminCompanyDetail {
    AdminCompanyDetail {
        id: d.id.into(),
        name: d.name.into(),
        subdomain: d.subdomain.into(),
        domain: d.domain.into(),
        logo: super::super::image::decode_pixel_buffer(&d.logo)
            .map(slint::Image::from_rgba8)
            .unwrap_or_default(),
        created_at: d.created_at.into(),
        active: d.active,
        document: d.document.into(),
        phone: d.phone.into(),
        whatsapp: d.whatsapp.into(),
        email: d.email.into(),
        address: d.address.into(),
        city_uf: d.city_uf.into(),
        plan: d.plan.into(),
        plan_amount: d.plan_amount.into(),
        status: d.status.into(),
        next_charge: d.next_charge.into(),
        discount: d.discount.into(),
        payment_method: d.payment_method.into(),
        invoices_total: d.invoices_total as i32,
        invoices_pending: d.invoices_pending as i32,
        orders_count: d.orders_count as i32,
        products_count: d.products_count as i32,
        customers_count: d.customers_count as i32,
        last_order_at: d.last_order_at.into(),
    }
}
