//! Recarga dos dados do painel: um GET por endpoint `/admin/*`, cache
//! atualizado e listas reaplicadas com a busca/filtro correntes.

use std::sync::Arc;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use tokio::sync::RwLock;

use crate::{AdminRevenuePoint, AdminState, MainWindow};

use super::cache::AdminCaches;
use super::dto::{AdminDto, AdminRoleDto, CompanyDto, OverviewDto, PlanDto, SubscriptionDto};
use super::filters::{
    apply_company_filter, apply_plan_filter, apply_role_filter, apply_sub_filter, apply_user_filter,
    set_plan_filter_options,
};
use super::http::get_json;

/// Tudo o que uma recarga traz do servidor.
struct AdminData {
    overview: Option<OverviewDto>,
    companies: Vec<CompanyDto>,
    subs: Vec<SubscriptionDto>,
    admins: Vec<AdminDto>,
    plans: Vec<PlanDto>,
    roles: Vec<AdminRoleDto>,
}

/// Carrega painel + empresas + assinaturas + administradores.
pub(super) fn setup_refresh(
    ui: &MainWindow,
    handle: &tokio::runtime::Handle,
    auth_token: &Arc<RwLock<Option<String>>>,
    server_url: &str,
    caches: &AdminCaches,
) {
    let ui_weak = ui.as_weak();
    let handle = handle.clone();
    let auth_token = auth_token.clone();
    let server_url = server_url.to_string();
    let caches = caches.clone();
    ui.global::<AdminState>().on_refresh(move || {
        let ui_weak = ui_weak.clone();
        let auth_token = auth_token.clone();
        let server_url = server_url.clone();
        let caches = caches.clone();
        handle.spawn(async move {
            let Some(token) = auth_token.read().await.clone() else { return };
            let data = fetch_all(&server_url, &token).await;
            let AdminData { overview, companies, subs, admins, plans, roles } = data;
            if let Ok(mut g) = caches.plans.lock() {
                *g = plans;
            }
            if let Ok(mut g) = caches.roles.lock() {
                *g = roles;
            }
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                if let Some(o) = overview {
                    apply_overview(&ui, o);
                }
                apply_lists(&ui, &caches, companies, subs, admins);
            });
        });
    });
}

/// Busca sequencial dos endpoints do painel (listas vazias em falha).
async fn fetch_all(server_url: &str, token: &str) -> AdminData {
    AdminData {
        overview: get_json(&format!("{server_url}/admin/overview"), token).await,
        companies: get_json(&format!("{server_url}/admin/companies"), token)
            .await
            .unwrap_or_default(),
        subs: get_json(&format!("{server_url}/admin/subscriptions"), token)
            .await
            .unwrap_or_default(),
        admins: get_json(&format!("{server_url}/admin/admins"), token)
            .await
            .unwrap_or_default(),
        plans: get_json(&format!("{server_url}/admin/plans"), token)
            .await
            .unwrap_or_default(),
        roles: get_json(&format!("{server_url}/admin/roles"), token)
            .await
            .unwrap_or_default(),
    }
}

/// Guarda as listas completas e exibe já filtradas pela busca/filtro
/// correntes (mantém o estado da UI no refresh).
fn apply_lists(
    ui: &MainWindow,
    caches: &AdminCaches,
    companies: Vec<CompanyDto>,
    subs: Vec<SubscriptionDto>,
    admins: Vec<AdminDto>,
) {
    if let Ok(mut c) = caches.companies.lock() {
        *c = companies;
    }
    apply_company_filter(ui, &caches.companies);

    if let Ok(mut s) = caches.subs.lock() {
        *s = subs;
    }
    apply_sub_filter(ui, &caches.subs);

    if let Ok(mut u) = caches.users.lock() {
        *u = admins;
    }
    apply_user_filter(ui, &caches.users);

    // Lista de planos já filtrada pela busca corrente.
    apply_plan_filter(ui, &caches.plans);
    // Opções do filtro "Planos" = planos cadastrados.
    if let Ok(g) = caches.plans.lock() {
        set_plan_filter_options(ui, &g);
    }
    // Funções + opções do seletor de função no usuário.
    apply_role_filter(ui, &caches.roles);
}

/// Reflete o resumo (GET /admin/overview) nos cards e no gráfico.
fn apply_overview(ui: &MainWindow, o: OverviewDto) {
    apply_counters(ui, &o);
    apply_revenue_chart(ui, o.year, o.prev_year, o.revenue_months);
}

/// Contadores e valores consolidados dos cards do topo.
fn apply_counters(ui: &MainWindow, o: &OverviewDto) {
    let g = ui.global::<AdminState>();
    g.set_companies_count(o.companies as i32);
    g.set_active_companies(o.active_companies as i32);
    g.set_suspended_companies(o.suspended_companies as i32);
    g.set_active_subs(o.active_subscriptions as i32);
    g.set_overdue_subs(o.overdue_subscriptions as i32);
    g.set_cancelled_subs(o.cancelled_subscriptions as i32);
    g.set_inactive_subs(o.inactive_subscriptions as i32);
    g.set_admins_count(o.super_admins as i32);
    g.set_new_companies_month(o.new_companies_month as i32);
    g.set_referrals(o.referrals as i32);
    g.set_mrr(SharedString::from(o.mrr.as_str()));
    g.set_annual_revenue(SharedString::from(o.annual_revenue.as_str()));
    g.set_arpa(SharedString::from(o.arpa.as_str()));
    g.set_top_plan(SharedString::from(o.top_plan.as_str()));
}

/// Gráfico de receita anual (jan–dez, ano atual × anterior).
/// Escala = maior valor entre os dois anos.
fn apply_revenue_chart(
    ui: &MainWindow,
    year: i32,
    prev_year: i32,
    months: Vec<super::dto::RevenuePointDto>,
) {
    let g = ui.global::<AdminState>();
    g.set_current_year(year);
    g.set_prev_year(prev_year);
    let max = months
        .iter()
        .flat_map(|p| [p.current, p.previous])
        .fold(0.0_f64, f64::max);
    g.set_revenue_max(max as f32);
    let points: Vec<AdminRevenuePoint> = months
        .into_iter()
        .map(|p| AdminRevenuePoint {
            label: p.label.into(),
            current: p.current as f32,
            previous: p.previous as f32,
            current_amount: p.current_brl.into(),
            previous_amount: p.previous_brl.into(),
        })
        .collect();
    g.set_revenue_points(ModelRc::new(VecModel::from(points)));
}
