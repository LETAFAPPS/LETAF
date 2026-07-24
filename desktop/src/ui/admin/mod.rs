//! Painel do administrador (super admin) — camada de UI do desktop.
//!
//! Regras (AI_RULES.md §1, §3, §11):
//! - UI burra: aqui só chamamos as rotas `/admin/*` (online) e refletimos
//!   o resultado nos modelos Slint. A autoridade é o backend, que exige
//!   `role == super_admin` em toda rota.
//! - Diferente das telas da loja (offline-first/SQLite), o painel é
//!   inerentemente ONLINE: o super admin gere dados cross-tenant do
//!   servidor, não há espelho local.

use std::sync::{Arc, Mutex};

use serde::de::DeserializeOwned;
use serde::Deserialize;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use tokio::sync::RwLock;

use crate::AdminState;
use crate::{
    AdminAuditRow, AdminCompanyDetail, AdminCompanyOrderRow, AdminCompanyRow, AdminInvoiceRow,
    AdminPlanRow,
    AdminSubscriptionRow,
    AdminUserRow, MainWindow,
    HTTP_CLIENT,
};

use super::helpers::show_toast;

// ── DTOs espelhando as respostas JSON do servidor (routes/admin.rs) ──────
#[derive(Deserialize)]
struct OverviewDto {
    companies: i64,
    active_subscriptions: i64,
    overdue_subscriptions: i64,
    cancelled_subscriptions: i64,
    super_admins: i64,
    new_companies_month: i64,
    mrr: String,
}

#[derive(Deserialize, Clone)]
struct CompanyDto {
    id: String,
    name: String,
    subdomain: String,
    created_at: String,
    plan: String,
    status: String,
    active: bool,
}

#[derive(Deserialize)]
struct InvoiceDto {
    id: String,
    number: String,
    description: String,
    amount: String,
    status: String,
    issued_at: String,
    paid_at: String,
    method: String,
}

#[derive(Deserialize, Clone)]
struct SubscriptionDto {
    company_id: String,
    company_name: String,
    plan: String,
    status: String,
    next_charge: String,
    payment_kind: String,
    discount: String,
}

#[derive(Deserialize)]
struct CompanyOrderDto {
    number: i64,
    status: String,
    total: String,
    at: String,
}

#[derive(Deserialize)]
struct CompanyDetailDto {
    id: String,
    name: String,
    subdomain: String,
    created_at: String,
    active: bool,
    document: String,
    phone: String,
    whatsapp: String,
    email: String,
    address: String,
    city_uf: String,
    plan: String,
    plan_amount: String,
    status: String,
    next_charge: String,
    discount: String,
    payment_method: String,
    invoices_total: i64,
    invoices_pending: i64,
    orders_count: i64,
    products_count: i64,
    customers_count: i64,
    last_order_at: String,
}

#[derive(Deserialize)]
struct AuditDto {
    actor: String,
    action: String,
    target: String,
    details: String,
    at: String,
}

#[derive(Deserialize)]
struct AdminDto {
    id: String,
    name: String,
    email: String,
}

#[derive(Deserialize, Clone, Default)]
struct PlanDto {
    id: String,
    name: String,
    amount: f64,
    period_months: i32,
    trial_days: i32,
    description: String,
    highlight_label: String,
    active: bool,
    monthly_price: f64,
    /// Quantas empresas usam este plano (0 para payloads sem o campo).
    #[serde(default)]
    companies: i64,
}

/// Formata um valor em reais ("R$ 2.000,00"). Delega ao helper canônico
/// (a versão anterior não tinha separador de milhar — AI_RULES §8).
fn brl(v: f64) -> String {
    crate::format::money_br_f64(v)
}

/// Cache dos planos crus (para o "editar" preencher o form com os valores
/// numéricos, já que o modelo Slint só guarda os textos de exibição).
type PlansCache = Arc<Mutex<Vec<PlanDto>>>;
/// Listas COMPLETAS vindas da API; a busca/filtro é aplicada sobre elas
/// para montar o modelo exibido (§13 — sem novo round-trip por tecla).
type CompaniesCache = Arc<Mutex<Vec<CompanyDto>>>;
type SubsCache = Arc<Mutex<Vec<SubscriptionDto>>>;

/// `true` se `haystack` contém `needle` ignorando caixa (busca simples).
fn matches(haystack: &str, needle: &str) -> bool {
    needle.is_empty() || haystack.to_lowercase().contains(&needle.to_lowercase())
}

/// Aplica busca (nome/subdomínio) + filtro de acesso às empresas.
fn apply_company_filter(ui: &MainWindow, cache: &CompaniesCache) {
    let search = ui.global::<AdminState>().get_company_search().to_string();
    let filter = ui.global::<AdminState>().get_company_filter().to_string();
    let Ok(all) = cache.lock() else { return };
    let rows: Vec<AdminCompanyRow> = all
        .iter()
        .filter(|c| matches(&c.name, &search) || matches(&c.subdomain, &search))
        .filter(|c| match filter.as_str() {
            "active" => c.active,
            "suspended" => !c.active,
            _ => true,
        })
        .map(|c| AdminCompanyRow {
            id: c.id.clone().into(),
            name: c.name.clone().into(),
            subdomain: c.subdomain.clone().into(),
            created_at: c.created_at.clone().into(),
            plan: c.plan.clone().into(),
            status: c.status.clone().into(),
            active: c.active,
        })
        .collect();
    ui.global::<AdminState>().set_companies(ModelRc::new(VecModel::from(rows)));
}

/// Aplica busca (nome da empresa) + filtro de status às assinaturas.
fn apply_sub_filter(ui: &MainWindow, cache: &SubsCache) {
    let search = ui.global::<AdminState>().get_sub_search().to_string();
    let filter = ui.global::<AdminState>().get_sub_filter().to_string();
    let Ok(all) = cache.lock() else { return };
    let rows: Vec<AdminSubscriptionRow> = all
        .iter()
        .filter(|s| matches(&s.company_name, &search))
        .filter(|s| filter == "all" || s.status == filter)
        .map(|s| AdminSubscriptionRow {
            company_id: s.company_id.clone().into(),
            company_name: s.company_name.clone().into(),
            plan: s.plan.clone().into(),
            status: s.status.clone().into(),
            next_charge: s.next_charge.clone().into(),
            payment_kind: s.payment_kind.clone().into(),
            discount: s.discount.clone().into(),
        })
        .collect();
    ui.global::<AdminState>().set_subscriptions(ModelRc::new(VecModel::from(rows)));
}

/// Registra todos os callbacks do painel do administrador.
pub(crate) fn setup_admin(
    ui: &MainWindow,
    handle: &tokio::runtime::Handle,
    auth_token: Arc<RwLock<Option<String>>>,
    server_url: String,
) {
    let plans_cache: PlansCache = Arc::new(Mutex::new(Vec::new()));
    let companies_cache: CompaniesCache = Arc::new(Mutex::new(Vec::new()));
    let subs_cache: SubsCache = Arc::new(Mutex::new(Vec::new()));
    setup_refresh(
        ui,
        handle,
        &auth_token,
        &server_url,
        &plans_cache,
        &companies_cache,
        &subs_cache,
    );
    setup_filters(ui, &companies_cache, &subs_cache);
    setup_form(ui);
    setup_persist(ui, handle, &auth_token, &server_url);
    setup_company_persist(ui, handle, &auth_token, &server_url);
    setup_company_pickers(ui, handle);
    setup_company_cep(ui, handle);
    setup_company_form_helpers(ui);
    setup_plan_form(ui, &plans_cache);
    setup_plan_persist(ui, handle, &auth_token, &server_url);
}

/// GET autenticado → desserializa em `T`. `None` em qualquer falha.
async fn get_json<T: DeserializeOwned>(url: &str, token: &str) -> Option<T> {
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

/// Busca/filtro das listas — reaplica sobre o cache, sem ir à rede.
fn setup_filters(ui: &MainWindow, companies_cache: &CompaniesCache, subs_cache: &SubsCache) {
    {
        let ui_weak = ui.as_weak();
        let cache = companies_cache.clone();
        ui.global::<AdminState>().on_filter_companies(move || {
            if let Some(ui) = ui_weak.upgrade() {
                apply_company_filter(&ui, &cache);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let cache = subs_cache.clone();
        ui.global::<AdminState>().on_filter_subscriptions(move || {
            if let Some(ui) = ui_weak.upgrade() {
                apply_sub_filter(&ui, &cache);
            }
        });
    }
}

/// Carrega painel + empresas + assinaturas + administradores.
#[allow(clippy::too_many_arguments)]
fn setup_refresh(
    ui: &MainWindow,
    handle: &tokio::runtime::Handle,
    auth_token: &Arc<RwLock<Option<String>>>,
    server_url: &str,
    plans_cache: &PlansCache,
    companies_cache: &CompaniesCache,
    subs_cache: &SubsCache,
) {
    let ui_weak = ui.as_weak();
    let handle = handle.clone();
    let auth_token = auth_token.clone();
    let server_url = server_url.to_string();
    let plans_cache = plans_cache.clone();
    let companies_cache = companies_cache.clone();
    let subs_cache = subs_cache.clone();
    ui.global::<AdminState>().on_refresh(move || {
        let ui_weak = ui_weak.clone();
        let auth_token = auth_token.clone();
        let server_url = server_url.clone();
        let plans_cache = plans_cache.clone();
        let companies_cache = companies_cache.clone();
        let subs_cache = subs_cache.clone();
        handle.spawn(async move {
            let Some(token) = auth_token.read().await.clone() else { return };

            let overview: Option<OverviewDto> =
                get_json(&format!("{server_url}/admin/overview"), &token).await;
            let companies: Vec<CompanyDto> =
                get_json(&format!("{server_url}/admin/companies"), &token)
                    .await
                    .unwrap_or_default();
            let subs: Vec<SubscriptionDto> =
                get_json(&format!("{server_url}/admin/subscriptions"), &token)
                    .await
                    .unwrap_or_default();
            let admins: Vec<AdminDto> =
                get_json(&format!("{server_url}/admin/admins"), &token)
                    .await
                    .unwrap_or_default();
            let plans: Vec<PlanDto> =
                get_json(&format!("{server_url}/admin/plans"), &token)
                    .await
                    .unwrap_or_default();
            let audit: Vec<AuditDto> =
                get_json(&format!("{server_url}/admin/audit"), &token)
                    .await
                    .unwrap_or_default();
            if let Ok(mut g) = plans_cache.lock() {
                *g = plans.clone();
            }

            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                if let Some(o) = overview {
                    ui.global::<AdminState>().set_companies_count(o.companies as i32);
                    ui.global::<AdminState>().set_active_subs(o.active_subscriptions as i32);
                    ui.global::<AdminState>().set_overdue_subs(o.overdue_subscriptions as i32);
                    ui.global::<AdminState>().set_cancelled_subs(o.cancelled_subscriptions as i32);
                    ui.global::<AdminState>().set_admins_count(o.super_admins as i32);
                    ui.global::<AdminState>().set_new_companies_month(o.new_companies_month as i32);
                    ui.global::<AdminState>().set_mrr(SharedString::from(o.mrr));
                }
                // Guarda as listas completas e exibe já filtradas pela
                // busca/filtro correntes (mantém o estado da UI no refresh).
                if let Ok(mut c) = companies_cache.lock() {
                    *c = companies;
                }
                apply_company_filter(&ui, &companies_cache);

                if let Ok(mut s) = subs_cache.lock() {
                    *s = subs;
                }
                apply_sub_filter(&ui, &subs_cache);

                let admin_rows: Vec<AdminUserRow> = admins
                    .into_iter()
                    .map(|a| AdminUserRow {
                        id: a.id.into(),
                        name: a.name.into(),
                        email: a.email.into(),
                    })
                    .collect();
                ui.global::<AdminState>().set_users(ModelRc::new(VecModel::from(admin_rows)));

                let plan_rows: Vec<AdminPlanRow> = plans
                    .into_iter()
                    .map(|p| AdminPlanRow {
                        id: p.id.into(),
                        name: p.name.into(),
                        amount_display: brl(p.amount).into(),
                        monthly_display: format!("{}/mês", brl(p.monthly_price)).into(),
                        period_months: p.period_months,
                        trial_days: p.trial_days,
                        description: p.description.into(),
                        highlight_label: p.highlight_label.into(),
                        active: p.active,
                        companies: p.companies as i32,
                    })
                    .collect();
                ui.global::<AdminState>().set_plans(ModelRc::new(VecModel::from(plan_rows)));

                let audit_rows: Vec<AdminAuditRow> = audit
                    .into_iter()
                    .map(|a| AdminAuditRow {
                        actor: a.actor.into(),
                        action: a.action.into(),
                        target: a.target.into(),
                        details: a.details.into(),
                        at: a.at.into(),
                    })
                    .collect();
                ui.global::<AdminState>().set_audit_entries(ModelRc::new(VecModel::from(audit_rows)));
            });
        });
    });
}

/// Formulário de plano: novo (limpa) e editar (preenche do cache).
fn setup_plan_form(ui: &MainWindow, plans_cache: &PlansCache) {
    {
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
        });
    }
    {
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
                ui.global::<AdminState>().set_plan_period(p.period_months.to_string().into());
                ui.global::<AdminState>().set_plan_trial(p.trial_days.to_string().into());
                ui.global::<AdminState>().set_plan_description(p.description.clone().into());
                ui.global::<AdminState>().set_plan_highlight(p.highlight_label.clone().into());
                ui.global::<AdminState>().set_plan_active(p.active);
            }
        });
    }
}

/// Salvar (criar/atualizar) e excluir plano.
fn setup_plan_persist(
    ui: &MainWindow,
    handle: &tokio::runtime::Handle,
    auth_token: &Arc<RwLock<Option<String>>>,
    server_url: &str,
) {
    // Salvar.
    {
        let ui_weak = ui.as_weak();
        let handle = handle.clone();
        let auth_token = auth_token.clone();
        let server_url = server_url.to_string();
        ui.global::<AdminState>().on_plan_save(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            let id = ui.global::<AdminState>().get_plan_id().to_string();
            let name = ui.global::<AdminState>().get_plan_name().trim().to_string();
            // Aceita vírgula ou ponto como separador decimal.
            let amount: f64 = ui
                .global::<AdminState>().get_plan_amount()
                .replace('.', "")
                .replace(',', ".")
                .trim()
                .parse()
                .unwrap_or(0.0);
            let period: i32 = ui.global::<AdminState>().get_plan_period().trim().parse().unwrap_or(0);
            let trial: i32 = ui.global::<AdminState>().get_plan_trial().trim().parse().unwrap_or(0);
            let description = ui.global::<AdminState>().get_plan_description().to_string();
            let highlight = ui.global::<AdminState>().get_plan_highlight().to_string();
            let active = ui.global::<AdminState>().get_plan_active();
            if name.is_empty() {
                show_toast(&ui, "Informe o nome do plano", "error");
                return;
            }
            if amount <= 0.0 || period < 1 {
                show_toast(&ui, "Valor e período devem ser válidos", "error");
                return;
            }
            let body = serde_json::json!({
                "name": name, "amount": amount, "period_months": period,
                "trial_days": trial, "description": description,
                "highlight_label": highlight, "active": active,
            });
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
                report(ui_weak, result, "Plano Salvo").await;
            });
        });
    }
    // Excluir.
    {
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
}

/// Callbacks síncronos do formulário (novo / editar → preenche campos).
fn setup_form(ui: &MainWindow) {
    // Novo: limpa o formulário.
    {
        let ui_weak = ui.as_weak();
        ui.global::<AdminState>().on_new_user(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            ui.global::<AdminState>().set_form_id(SharedString::new());
            ui.global::<AdminState>().set_form_name(SharedString::new());
            ui.global::<AdminState>().set_form_email(SharedString::new());
            ui.global::<AdminState>().set_form_password(SharedString::new());
        });
    }
    // Editar: acha o admin no modelo e preenche (senha em branco = manter).
    {
        let ui_weak = ui.as_weak();
        ui.global::<AdminState>().on_edit_user(move |id| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let users = ui.global::<AdminState>().get_users();
            if let Some(u) = users.iter().find(|u| u.id == id) {
                ui.global::<AdminState>().set_form_id(u.id.clone());
                ui.global::<AdminState>().set_form_name(u.name.clone());
                ui.global::<AdminState>().set_form_email(u.email.clone());
                ui.global::<AdminState>().set_form_password(SharedString::new());
            }
        });
    }
}

/// Salvar (criar/atualizar) e excluir administrador.
fn setup_persist(
    ui: &MainWindow,
    handle: &tokio::runtime::Handle,
    auth_token: &Arc<RwLock<Option<String>>>,
    server_url: &str,
) {
    // Salvar.
    {
        let ui_weak = ui.as_weak();
        let handle = handle.clone();
        let auth_token = auth_token.clone();
        let server_url = server_url.to_string();
        ui.global::<AdminState>().on_save_user(move |id, name, email, password| {
            let name = name.trim().to_string();
            let email = email.trim().to_string();
            if name.is_empty() || email.is_empty() {
                if let Some(ui) = ui_weak.upgrade() {
                    show_toast(&ui, "Informe nome e e-mail", "error");
                }
                return;
            }
            if id.is_empty() && password.trim().is_empty() {
                if let Some(ui) = ui_weak.upgrade() {
                    show_toast(&ui, "Defina uma senha para o novo administrador", "error");
                }
                return;
            }
            let id = id.to_string();
            let password = password.to_string();
            let ui_weak = ui_weak.clone();
            let auth_token = auth_token.clone();
            let server_url = server_url.clone();
            handle.spawn(async move {
                let Some(token) = auth_token.read().await.clone() else { return };
                let result = if id.is_empty() {
                    let body = serde_json::json!({ "name": name, "email": email, "password": password });
                    HTTP_CLIENT
                        .post(format!("{server_url}/admin/admins"))
                        .bearer_auth(&token)
                        .json(&body)
                        .send()
                        .await
                } else {
                    let pw = if password.trim().is_empty() { None } else { Some(password) };
                    let body = serde_json::json!({ "name": name, "email": email, "password": pw });
                    HTTP_CLIENT
                        .put(format!("{server_url}/admin/admins/{id}"))
                        .bearer_auth(&token)
                        .json(&body)
                        .send()
                        .await
                };
                report(ui_weak, result, "Administrador Salvo").await;
            });
        });
    }
    // Excluir.
    {
        let ui_weak = ui.as_weak();
        let handle = handle.clone();
        let auth_token = auth_token.clone();
        let server_url = server_url.to_string();
        ui.global::<AdminState>().on_delete_user(move |id| {
            let id = id.to_string();
            let ui_weak = ui_weak.clone();
            let auth_token = auth_token.clone();
            let server_url = server_url.clone();
            handle.spawn(async move {
                let Some(token) = auth_token.read().await.clone() else { return };
                let result = HTTP_CLIENT
                    .delete(format!("{server_url}/admin/admins/{id}"))
                    .bearer_auth(&token)
                    .send()
                    .await;
                report(ui_weak, result, "Administrador Removido").await;
            });
        });
    }
    // Detalhe consolidado de uma empresa (modal de suporte).
    {
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
                    ui.global::<AdminState>().set_detail(AdminCompanyDetail {
                        id: d.id.into(),
                        name: d.name.into(),
                        subdomain: d.subdomain.into(),
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
                    });
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
                });
            });
        });
    }
    // Faturas: carregar o histórico da empresa em edição.
    {
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
                    let rows: Vec<AdminInvoiceRow> = invoices
                        .into_iter()
                        .map(|i| AdminInvoiceRow {
                            id: i.id.into(),
                            number: i.number.into(),
                            description: i.description.into(),
                            amount: i.amount.into(),
                            status: i.status.into(),
                            issued_at: i.issued_at.into(),
                            paid_at: i.paid_at.into(),
                            method: i.method.into(),
                        })
                        .collect();
                    ui.global::<AdminState>().set_invoices(ModelRc::new(VecModel::from(rows)));
                });
            });
        });
    }
    // Faturas: baixa manual de uma fatura pendente.
    {
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
                let outcome = write_outcome(result).await;
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = ui_weak.upgrade() else { return };
                    match outcome {
                        Ok(()) => {
                            show_toast(&ui, "Fatura marcada como paga", "success");
                            // Recarrega faturas e listas (o status da
                            // assinatura pode ter voltado a "ativa").
                            ui.global::<AdminState>().invoke_load_invoices(SharedString::from(company_id.as_str()));
                            ui.global::<AdminState>().invoke_refresh();
                        }
                        Err(msg) => show_toast(&ui, &msg, "error"),
                    }
                });
            });
        });
    }
    // Suspender/reativar acesso do tenant (super admin).
    {
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
    // Excluir empresa (soft delete via API admin, cross-tenant).
    {
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
                report(ui_weak, result, "Empresa Excluída").await;
            });
        });
    }
    // Gestão da assinatura de uma empresa (plano, status e desconto).
    {
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
            let plan = ui.global::<AdminState>().get_sub_edit_plan().to_string();
            let status = ui.global::<AdminState>().get_sub_edit_status().to_string();
            // Aceita vírgula ou ponto como separador decimal.
            let discount: f64 = ui
                .global::<AdminState>().get_sub_edit_discount()
                .replace('.', "")
                .replace(',', ".")
                .trim()
                .parse()
                .unwrap_or(0.0);
            ui.global::<AdminState>().set_sub_edit_busy(true);
            let body = serde_json::json!({
                "plan": plan, "status": status, "discount": discount,
            });
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
}

/// Converte a resposta HTTP num `Result<(), String>`, extraindo a mensagem
/// de erro do corpo quando o servidor rejeita (4xx). Fonte única do parser
/// de erro para os `report*` (§8).
async fn write_outcome(result: Result<reqwest::Response, reqwest::Error>) -> Result<(), String> {
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

/// Feedback + refresh após salvar/excluir um administrador.
async fn report(
    ui_weak: slint::Weak<MainWindow>,
    result: Result<reqwest::Response, reqwest::Error>,
    ok_msg: &'static str,
) {
    let outcome = write_outcome(result).await;
    let _ = slint::invoke_from_event_loop(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        match outcome {
            Ok(()) => {
                show_toast(&ui, ok_msg, "success");
                // Limpa o formulário e recarrega as listas.
                ui.global::<AdminState>().set_form_id(SharedString::new());
                ui.global::<AdminState>().set_form_name(SharedString::new());
                ui.global::<AdminState>().set_form_email(SharedString::new());
                ui.global::<AdminState>().set_form_password(SharedString::new());
                ui.global::<AdminState>().invoke_refresh();
            }
            Err(msg) => show_toast(&ui, &msg, "error"),
        }
    });
}

/// Cadastro de estabelecimento (empresa + admin inicial + infos) via
/// POST /admin/companies. O form é grande → lê as propriedades da UI
/// (sem callback com dezenas de args).
fn setup_company_persist(
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
        let name = ui.global::<AdminState>().get_company_form_name().trim().to_string();
        let subdomain = ui.global::<AdminState>().get_company_form_subdomain().trim().to_lowercase();
        let admin_name = ui.global::<AdminState>().get_company_form_admin_name().trim().to_string();
        let admin_email = ui.global::<AdminState>().get_company_form_admin_email().trim().to_string();
        let admin_password = ui.global::<AdminState>().get_company_form_admin_password().to_string();
        if name.is_empty() || subdomain.is_empty() || admin_name.is_empty() || admin_email.is_empty() {
            show_toast(&ui, "Preencha empresa, subdomínio, nome e e-mail do admin", "error");
            return;
        }
        if admin_password.trim().is_empty() {
            show_toast(&ui, "Defina uma senha para o administrador", "error");
            return;
        }
        let discount = parse_money_br(&ui.global::<AdminState>().get_company_form_discount());
        let body = serde_json::json!({
            "name": name,
            "subdomain": subdomain,
            "admin_name": admin_name,
            "admin_email": admin_email,
            "admin_password": admin_password,
            "phone": ui.global::<AdminState>().get_company_form_phone().trim(),
            "whatsapp": ui.global::<AdminState>().get_company_form_whatsapp().trim(),
            "email": ui.global::<AdminState>().get_company_form_email().trim(),
            "document": ui.global::<AdminState>().get_company_form_document().trim(),
            "address": ui.global::<AdminState>().get_company_form_address().trim(),
            "neighborhood": ui.global::<AdminState>().get_company_form_neighborhood().trim(),
            "zip_code": ui.global::<AdminState>().get_company_form_zip().trim(),
            "city": ui.global::<AdminState>().get_company_form_city().trim(),
            "uf": ui.global::<AdminState>().get_company_form_uf().trim(),
            "logo_data": ui.global::<AdminState>().get_company_form_logo_data().to_string(),
            "cover_data": ui.global::<AdminState>().get_company_form_cover_data().to_string(),
            "plan_discount": discount,
        });
        let ui_weak = ui.as_weak();
        let auth_token = auth_token.clone();
        let server_url = server_url.clone();
        handle.spawn(async move {
            let Some(token) = auth_token.read().await.clone() else { return };
            let result = HTTP_CLIENT
                .post(format!("{server_url}/admin/companies"))
                .bearer_auth(&token)
                .json(&body)
                .send()
                .await;
            let outcome = write_outcome(result).await;
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                match outcome {
                    Ok(()) => {
                        show_toast(&ui, "Estabelecimento Cadastrado", "success");
                        clear_company_form(&ui);
                        // Volta à lista e limpa os erros (senão o formulário
                        // fica todo vermelho com os campos já zerados).
                        ui.global::<AdminState>().set_company_form_attempted(false);
                        ui.global::<AdminState>().set_company_show_form(false);
                        ui.global::<AdminState>().invoke_refresh();
                    }
                    Err(msg) => show_toast(&ui, &msg, "error"),
                }
            });
        });
    });

    // "+": abre um cadastro LIMPO (sem sobras de uma edição anterior nem
    // erros de uma tentativa passada).
    {
        let ui_weak = ui.as_weak();
        ui.global::<AdminState>().on_company_new_form(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            clear_company_form(&ui);
            ui.global::<AdminState>().set_company_form_attempted(false);
            ui.global::<AdminState>().set_company_show_form(true);
        });
    }
}

/// Converte um valor monetário digitado (pt-BR ou simples) em `f64`.
/// Aceita "30", "30,00", "1.234,56", "R$ 30". Inválido → 0.
fn parse_money_br(raw: &str) -> f64 {
    let cleaned = raw.trim().replace("R$", "").replace(' ', "");
    let normalized = if cleaned.contains(',') {
        cleaned.replace('.', "").replace(',', ".")
    } else {
        cleaned
    };
    normalized.parse::<f64>().unwrap_or(0.0).max(0.0)
}

/// Limpa todos os campos do formulário de novo estabelecimento.
fn clear_company_form(ui: &MainWindow) {
    ui.global::<AdminState>().set_company_form_name(SharedString::new());
    ui.global::<AdminState>().set_company_form_subdomain(SharedString::new());
    ui.global::<AdminState>().set_company_form_admin_name(SharedString::new());
    ui.global::<AdminState>().set_company_form_admin_email(SharedString::new());
    ui.global::<AdminState>().set_company_form_admin_password(SharedString::new());
    ui.global::<AdminState>().set_company_form_phone(SharedString::new());
    ui.global::<AdminState>().set_company_form_whatsapp(SharedString::new());
    ui.global::<AdminState>().set_company_form_email(SharedString::new());
    ui.global::<AdminState>().set_company_form_document(SharedString::new());
    ui.global::<AdminState>().set_company_form_discount(SharedString::new());
    ui.global::<AdminState>().set_company_form_address(SharedString::new());
    ui.global::<AdminState>().set_company_form_neighborhood(SharedString::new());
    ui.global::<AdminState>().set_company_form_zip(SharedString::new());
    ui.global::<AdminState>().set_company_form_city(SharedString::new());
    ui.global::<AdminState>().set_company_form_uf(SharedString::new());
    ui.global::<AdminState>().set_company_form_logo_data(SharedString::new());
    ui.global::<AdminState>().set_company_form_cover_data(SharedString::new());
    ui.global::<AdminState>().set_company_form_logo_image(slint::Image::default());
    ui.global::<AdminState>().set_company_form_cover_image(slint::Image::default());
}

/// Seletores de logo/capa do novo estabelecimento (espelha Configurações).
fn setup_company_pickers(ui: &MainWindow, handle: &tokio::runtime::Handle) {
    // Logo (imagem menor).
    {
        let ui_weak = ui.as_weak();
        let handle = handle.clone();
        ui.global::<AdminState>().on_pick_company_logo(move || {
            let ui_weak = ui_weak.clone();
            handle.spawn_blocking(move || {
                let Some(path) = super::image::pick_image_file() else { return };
                let uw = ui_weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = uw.upgrade() { ui.global::<AdminState>().set_company_form_logo_loading(true); }
                });
                let encoded = super::image::process_image_file(&path);
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = ui_weak.upgrade() else { return };
                    ui.global::<AdminState>().set_company_form_logo_loading(false);
                    if let Some(enc) = encoded {
                        let buf = super::image::decode_pixel_buffer(&enc);
                        ui.global::<AdminState>().set_company_form_logo_image(buf.map(slint::Image::from_rgba8).unwrap_or_default());
                        ui.global::<AdminState>().set_company_form_logo_data(SharedString::from(enc));
                    }
                });
            });
        });
    }
    // Capa (imagem maior).
    {
        let ui_weak = ui.as_weak();
        let handle = handle.clone();
        ui.global::<AdminState>().on_pick_company_cover(move || {
            let ui_weak = ui_weak.clone();
            handle.spawn_blocking(move || {
                let Some(path) = super::image::pick_image_file() else { return };
                let uw = ui_weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = uw.upgrade() { ui.global::<AdminState>().set_company_form_cover_loading(true); }
                });
                let encoded = super::image::process_image_file_large(&path);
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = ui_weak.upgrade() else { return };
                    ui.global::<AdminState>().set_company_form_cover_loading(false);
                    if let Some(enc) = encoded {
                        let buf = super::image::decode_pixel_buffer(&enc);
                        ui.global::<AdminState>().set_company_form_cover_image(buf.map(slint::Image::from_rgba8).unwrap_or_default());
                        ui.global::<AdminState>().set_company_form_cover_data(SharedString::from(enc));
                    }
                });
            });
        });
    }
}

/// Consulta o CEP (ViaCEP) e preenche cidade/UF. É conveniência de UX: o
/// operador ainda pode editar; o backend não depende disto.
///
/// Regras (§1/§3): a busca fica no Rust, não na UI. Falha silenciosa
/// (rede/CEP inexistente) — o operador digita manualmente.
/// Máscara e verificação dos campos do cadastro (feedback de UX; §11).
/// Puras — o Slint as chama em expressões de propriedade.
fn setup_company_form_helpers(ui: &MainWindow) {
    use crate::format::{field_error, format_document, format_money_input, format_phone,
        format_zip_code, sanitize_subdomain};
    ui.global::<AdminState>().on_apply_mask(|kind, value| {
        let out = match kind.as_str() {
            "document" => format_document(&value),
            "phone" => format_phone(&value),
            "cep" => format_zip_code(&value),
            "money" => format_money_input(&value),
            "subdomain" => sanitize_subdomain(&value),
            _ => value.to_string(),
        };
        SharedString::from(out)
    });
    ui.global::<AdminState>().on_field_error(|rule, value| {
        SharedString::from(field_error(&rule, &value))
    });
}

fn setup_company_cep(ui: &MainWindow, handle: &tokio::runtime::Handle) {
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
            #[derive(serde::Deserialize)]
            struct ViaCep {
                #[serde(default)] localidade: String,
                #[serde(default)] uf: String,
                #[serde(default)] erro: bool,
            }
            let res: Option<ViaCep> = match HTTP_CLIENT
                .get(format!("https://viacep.com.br/ws/{digits}/json/"))
                .send()
                .await
            {
                Ok(r) if r.status().is_success() => r.json::<ViaCep>().await.ok(),
                _ => None,
            };
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                ui.global::<AdminState>().set_company_cep_loading(false);
                if let Some(v) = res {
                    if !v.erro {
                        if !v.localidade.is_empty() {
                            ui.global::<AdminState>().set_company_form_city(SharedString::from(v.localidade));
                        }
                        if !v.uf.is_empty() {
                            ui.global::<AdminState>().set_company_form_uf(SharedString::from(v.uf));
                        }
                    }
                }
            });
        });
    });
}

