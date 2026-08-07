//! Busca/filtro das listas do painel.
//!
//! Tudo é reaplicado sobre o cache em memória (§13 — sem ir à rede a cada
//! tecla) e o resultado vira o modelo exibido pelo Slint.

use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::{
    AdminBusinessTypeRow, AdminCompanyRow, AdminPlanRow, AdminRoleRow, AdminState,
    AdminSubscriptionRow, AdminUserRow, FilterOption, MainWindow,
};

use super::super::image::decode_pixel_buffer;
use super::cache::{
    AdminCaches, BusinessTypesCache, CompaniesCache, PlansCache, RolesCache, SubsCache, UsersCache,
};
use super::dto::{
    AdminDto, AdminRoleDto, BusinessTypeDto, CompanyDto, PlanDto, SubscriptionDto,
};
use super::format::{brl, plan_features};
use super::roles::screen_label;

/// Busca/filtro das listas — reaplica sobre o cache, sem ir à rede.
pub(super) fn setup_filters(ui: &MainWindow, caches: &AdminCaches) {
    {
        let ui_weak = ui.as_weak();
        let cache = caches.companies.clone();
        ui.global::<AdminState>().on_filter_companies(move || {
            if let Some(ui) = ui_weak.upgrade() {
                apply_company_filter(&ui, &cache);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let cache = caches.subs.clone();
        ui.global::<AdminState>().on_filter_subscriptions(move || {
            if let Some(ui) = ui_weak.upgrade() {
                apply_sub_filter(&ui, &cache);
            }
        });
    }
    {
        let ui_weak = ui.as_weak();
        let cache = caches.users.clone();
        ui.global::<AdminState>().on_filter_users(move || {
            if let Some(ui) = ui_weak.upgrade() {
                apply_user_filter(&ui, &cache);
            }
        });
    }
}

/// `true` se `haystack` contém `needle` ignorando caixa (busca simples).
pub(super) fn matches(haystack: &str, needle: &str) -> bool {
    needle.is_empty() || haystack.to_lowercase().contains(&needle.to_lowercase())
}

// ── Usuários (administradores) ───────────────────────────────────────────

/// Aplica busca (nome/e-mail) aos usuários — sobre o cache, sem ir à rede.
pub(super) fn apply_user_filter(ui: &MainWindow, cache: &UsersCache) {
    let search = ui.global::<AdminState>().get_user_search().to_string();
    let Ok(all) = cache.lock() else { return };
    let rows: Vec<AdminUserRow> = all
        .iter()
        .filter(|u| matches(&u.name, &search) || matches(&u.email, &search))
        .map(user_row)
        .collect();
    ui.global::<AdminState>().set_users(ModelRc::new(VecModel::from(rows)));
}

fn user_row(u: &AdminDto) -> AdminUserRow {
    AdminUserRow {
        id: u.id.clone().into(),
        name: u.name.clone().into(),
        email: u.email.clone().into(),
        role_id: u.role_id.clone().into(),
        role_name: u.role_name.clone().into(),
        avatar: decode_pixel_buffer(&u.avatar)
            .map(slint::Image::from_rgba8)
            .unwrap_or_default(),
        active: u.active,
    }
}

// ── Funções (permissões de tela) ─────────────────────────────────────────

/// Aplica busca (nome) às Funções + monta as opções de função para o seletor
/// de usuário (id→nome). Sobre o cache, sem ir à rede.
pub(super) fn apply_role_filter(ui: &MainWindow, cache: &RolesCache) {
    let search = ui.global::<AdminState>().get_role_search().to_string();
    let Ok(all) = cache.lock() else { return };
    let rows: Vec<AdminRoleRow> = all
        .iter()
        .filter(|r| matches(&r.name, &search))
        .map(role_row)
        .collect();
    ui.global::<AdminState>().set_admin_roles(ModelRc::new(VecModel::from(rows)));

    // Opções do seletor de função no cadastro de usuário (todas as funções).
    let opts: Vec<FilterOption> = all
        .iter()
        .map(|r| FilterOption { key: r.id.clone().into(), label: r.name.clone().into() })
        .collect();
    ui.global::<AdminState>().set_admin_role_options(ModelRc::new(VecModel::from(opts)));
}

fn role_row(r: &AdminRoleDto) -> AdminRoleRow {
    let labels: Vec<&str> = r.screens.iter().map(|s| screen_label(s)).collect();
    AdminRoleRow {
        id: r.id.clone().into(),
        name: r.name.clone().into(),
        screens_summary: labels.join(" · ").into(),
        screen_count: r.screens.len() as i32,
        users: r.users as i32,
    }
}

// ── Empresas ─────────────────────────────────────────────────────────────

/// Aplica busca (nome/subdomínio) + filtro de acesso às empresas.
pub(super) fn apply_company_filter(ui: &MainWindow, cache: &CompaniesCache) {
    let search = ui.global::<AdminState>().get_company_search().to_string();
    let filter = ui.global::<AdminState>().get_company_filter().to_string();
    let plan_filter = ui.global::<AdminState>().get_company_plan_filter().to_string();
    let Ok(all) = cache.lock() else { return };
    let rows: Vec<AdminCompanyRow> = all
        .iter()
        // Busca por nome, subdomínio, cidade ou proprietário.
        .filter(|c| {
            matches(&c.name, &search)
                || matches(&c.subdomain, &search)
                || matches(&c.city, &search)
                || matches(&c.owner, &search)
        })
        .filter(|c| match filter.as_str() {
            "active" => c.active,
            "suspended" => !c.active,
            _ => true,
        })
        // Filtro por plano (id do catálogo): "none" = sem plano.
        .filter(|c| match plan_filter.as_str() {
            "all" => true,
            "none" => c.plan_id.is_empty(),
            plan_id => c.plan_id == plan_id,
        })
        .map(company_row)
        .collect();
    ui.global::<AdminState>().set_companies(ModelRc::new(VecModel::from(rows)));

    let label = plan_filter_label(ui, &plan_filter);
    ui.global::<AdminState>().set_company_plan_filter_label(label.into());
}

fn company_row(c: &CompanyDto) -> AdminCompanyRow {
    AdminCompanyRow {
        id: c.id.clone().into(),
        name: c.name.clone().into(),
        subdomain: c.subdomain.clone().into(),
        created_at: c.created_at.clone().into(),
        plan: c.plan.clone().into(),
        plan_id: c.plan_id.clone().into(),
        status: c.status.clone().into(),
        active: c.active,
        domain: c.domain.clone().into(),
        city: c.city.clone().into(),
        owner: c.owner.clone().into(),
        owner_phone: c.owner_phone.clone().into(),
        logo: decode_pixel_buffer(&c.logo)
            .map(slint::Image::from_rgba8)
            .unwrap_or_default(),
        payment_kind: c.payment_kind.clone().into(),
        next_charge: c.next_charge.clone().into(),
        discount: c.discount.clone().into(),
        discount_name: c.discount_name.clone().into(),
    }
}

/// Rótulo do botão "Planos": acompanha a seleção (nome do plano ou "Planos").
fn plan_filter_label(ui: &MainWindow, plan_filter: &str) -> String {
    if plan_filter == "all" {
        return "Planos".to_string();
    }
    let opts = ui.global::<AdminState>().get_company_plan_filter_options();
    (0..opts.row_count())
        .filter_map(|i| opts.row_data(i))
        .find(|o| o.key.as_str() == plan_filter)
        .map(|o| o.label.to_string())
        .unwrap_or_else(|| "Planos".to_string())
}

// ── Assinaturas ──────────────────────────────────────────────────────────

/// Aplica busca (nome da empresa) + filtro de status às assinaturas.
pub(super) fn apply_sub_filter(ui: &MainWindow, cache: &SubsCache) {
    let search = ui.global::<AdminState>().get_sub_search().to_string();
    let filter = ui.global::<AdminState>().get_sub_filter().to_string();
    let plan_filter = ui.global::<AdminState>().get_sub_plan_filter().to_string();
    let Ok(all) = cache.lock() else { return };
    let rows: Vec<AdminSubscriptionRow> = all
        .iter()
        .filter(|s| matches(&s.company_name, &search))
        .filter(|s| filter == "all" || s.status == filter)
        // Filtro por plano: "none" = sem plano; senão casa pelo tipo.
        .filter(|s| match plan_filter.as_str() {
            "all" => true,
            "none" => s.plan.is_empty(),
            kind => s.plan == kind,
        })
        .map(sub_row)
        .collect();
    ui.global::<AdminState>().set_subscriptions(ModelRc::new(VecModel::from(rows)));

    let label = plan_filter_label(ui, &plan_filter);
    ui.global::<AdminState>().set_sub_plan_filter_label(label.into());
}

fn sub_row(s: &SubscriptionDto) -> AdminSubscriptionRow {
    AdminSubscriptionRow {
        company_id: s.company_id.clone().into(),
        company_name: s.company_name.clone().into(),
        logo: decode_pixel_buffer(&s.logo)
            .map(slint::Image::from_rgba8)
            .unwrap_or_default(),
        plan: s.plan.clone().into(),
        status: s.status.clone().into(),
        next_charge: s.next_charge.clone().into(),
        payment_kind: s.payment_kind.clone().into(),
        discount: s.discount.clone().into(),
    }
}

// ── Planos ───────────────────────────────────────────────────────────────

/// Aplica busca (nome) aos planos — reaplica sobre o cache, sem ir à rede.
pub(super) fn apply_plan_filter(ui: &MainWindow, cache: &PlansCache) {
    let search = ui.global::<AdminState>().get_plan_search().to_string();
    let Ok(all) = cache.lock() else { return };
    let rows: Vec<AdminPlanRow> = all
        .iter()
        .filter(|p| matches(&p.name, &search))
        .map(plan_row)
        .collect();
    ui.global::<AdminState>().set_plans(ModelRc::new(VecModel::from(rows)));
}

fn plan_row(p: &PlanDto) -> AdminPlanRow {
    AdminPlanRow {
        id: p.id.clone().into(),
        name: p.name.clone().into(),
        amount_display: brl(p.amount).into(),
        monthly_display: format!("{}/mês", brl(p.monthly_price)).into(),
        period_days: p.period_days,
        trial_days: p.trial_days,
        description: p.description.clone().into(),
        monthly_value: brl(p.monthly_price).into(),
        features: ModelRc::new(VecModel::from(plan_features(&p.description))),
        subscribers_display: p.companies.to_string().into(),
        cycle_display: cycle_display(p).into(),
        highlight_label: p.highlight_label.clone().into(),
        active: p.active,
        companies: p.companies as i32,
    }
}

/// Texto do ciclo de cobrança ("a cada 30 dias · 7 dias grátis").
fn cycle_display(p: &PlanDto) -> String {
    format!(
        "a cada {} {}{}",
        p.period_days,
        if p.period_days == 1 { "dia" } else { "dias" },
        if p.trial_days > 0 {
            format!(" · {} dias grátis", p.trial_days)
        } else {
            String::new()
        }
    )
}

// ── Tipos de empresa ───────────────────────────────────────────────────────

/// Aplica busca (nome) aos tipos de empresa — reaplica sobre o cache.
pub(super) fn apply_business_type_filter(ui: &MainWindow, cache: &BusinessTypesCache) {
    let search = ui.global::<AdminState>().get_business_type_search().to_string();
    let Ok(all) = cache.lock() else { return };
    let rows: Vec<AdminBusinessTypeRow> = all
        .iter()
        .filter(|b| matches(&b.name, &search))
        .map(business_type_row)
        .collect();
    ui.global::<AdminState>()
        .set_business_types(ModelRc::new(VecModel::from(rows)));
}

fn business_type_row(b: &BusinessTypeDto) -> AdminBusinessTypeRow {
    AdminBusinessTypeRow {
        id: b.id.clone().into(),
        name: b.name.clone().into(),
        description: b.description.clone().into(),
        active: b.active,
        sort_order: b.sort_order,
    }
}

/// Monta as opções de plano a partir do catálogo CADASTRADO (por plano, não
/// por tipo, já que o período agora é em DIAS):
/// - filtro "Planos" das Empresas: "Todos" + cada plano (key = id) + "Sem plano";
/// - seletor do cadastro/edição: só planos ATIVOS (key = id).
pub(super) fn set_plan_filter_options(ui: &MainWindow, plans: &[PlanDto]) {
    let mut opts: Vec<FilterOption> = vec![FilterOption {
        key: "all".into(),
        label: "Todos".into(),
    }];
    for p in plans {
        opts.push(FilterOption {
            key: p.id.clone().into(),
            label: p.name.clone().into(),
        });
    }
    opts.push(FilterOption {
        key: "none".into(),
        label: "Sem plano".into(),
    });
    ui.global::<AdminState>()
        .set_company_plan_filter_options(ModelRc::new(VecModel::from(opts)));

    // Seletor de plano no cadastro/edição: só planos ATIVOS (key = id).
    let form_opts: Vec<FilterOption> = plans
        .iter()
        .filter(|p| p.active)
        .map(|p| FilterOption {
            key: p.id.clone().into(),
            label: p.name.clone().into(),
        })
        .collect();
    ui.global::<AdminState>()
        .set_company_form_plan_options(ModelRc::new(VecModel::from(form_opts)));
}
