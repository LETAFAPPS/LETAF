//! DTOs espelhando as respostas JSON do servidor (`server/routes/admin.rs`).
//!
//! Só transporte: nenhuma regra de negócio vive aqui (AI_RULES.md §1/§3) —
//! os tipos existem para desserializar o que o backend já decidiu.

use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize)]
pub(super) struct OverviewDto {
    pub(super) companies: i64,
    #[serde(default)]
    pub(super) active_companies: i64,
    #[serde(default)]
    pub(super) suspended_companies: i64,
    pub(super) active_subscriptions: i64,
    pub(super) overdue_subscriptions: i64,
    pub(super) cancelled_subscriptions: i64,
    #[serde(default)]
    pub(super) inactive_subscriptions: i64,
    pub(super) super_admins: i64,
    pub(super) new_companies_month: i64,
    pub(super) mrr: String,
    #[serde(default = "default_money")]
    pub(super) annual_revenue: String,
    #[serde(default = "default_money")]
    pub(super) arpa: String,
    #[serde(default = "default_dash")]
    pub(super) top_plan: String,
    #[serde(default)]
    pub(super) referrals: i64,
    #[serde(default)]
    pub(super) year: i32,
    #[serde(default)]
    pub(super) prev_year: i32,
    #[serde(default)]
    pub(super) revenue_months: Vec<RevenuePointDto>,
}

fn default_money() -> String {
    "R$ 0,00".to_string()
}

fn default_dash() -> String {
    "—".to_string()
}

/// Um ponto do gráfico de receita anual (GET /admin/overview): mês +
/// valor do ano atual e do anterior (comparativo).
#[derive(Deserialize)]
pub(super) struct RevenuePointDto {
    pub(super) label: String,
    pub(super) current: f64,
    pub(super) previous: f64,
    pub(super) current_brl: String,
    pub(super) previous_brl: String,
}

#[derive(Deserialize, Clone)]
pub(super) struct CompanyDto {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) subdomain: String,
    pub(super) created_at: String,
    pub(super) plan: String,
    #[serde(default)]
    pub(super) plan_id: String,
    pub(super) status: String,
    pub(super) active: bool,
    #[serde(default)]
    pub(super) domain: String,
    #[serde(default)]
    pub(super) city: String,
    #[serde(default)]
    pub(super) owner: String,
    #[serde(default)]
    pub(super) owner_phone: String,
    /// Logo da empresa (base64) — decodificado para thumbnail no card.
    #[serde(default)]
    pub(super) logo: String,
    #[serde(default)]
    pub(super) payment_kind: String,
    #[serde(default)]
    pub(super) next_charge: String,
    #[serde(default)]
    pub(super) discount: String,
    #[serde(default)]
    pub(super) discount_name: String,
}

/// Dados brutos de uma empresa para pré-preencher o cadastro em edição
/// (GET /admin/companies/{id}/form).
#[derive(Deserialize)]
pub(super) struct CompanyFormDto {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) subdomain: String,
    pub(super) document: String,
    pub(super) phone: String,
    pub(super) whatsapp: String,
    pub(super) email: String,
    pub(super) address: String,
    pub(super) neighborhood: String,
    pub(super) zip_code: String,
    pub(super) city: String,
    pub(super) uf: String,
    #[serde(default)]
    pub(super) location_url: Option<String>,
    pub(super) logo_data: String,
    pub(super) cover_data: String,
    pub(super) discount: f64,
    #[serde(default)]
    pub(super) plan: String,
    #[serde(default)]
    pub(super) trial_days: i32,
    pub(super) owner_name: String,
    pub(super) owner_email: String,
    pub(super) owner_phone: String,
}

#[derive(Deserialize)]
pub(super) struct InvoiceDto {
    pub(super) id: String,
    pub(super) number: String,
    pub(super) description: String,
    pub(super) amount: String,
    pub(super) status: String,
    pub(super) issued_at: String,
    pub(super) paid_at: String,
    pub(super) method: String,
}

#[derive(Deserialize, Clone)]
pub(super) struct SubscriptionDto {
    pub(super) company_id: String,
    pub(super) company_name: String,
    #[serde(default)]
    pub(super) logo: String,
    pub(super) plan: String,
    pub(super) status: String,
    pub(super) next_charge: String,
    pub(super) payment_kind: String,
    pub(super) discount: String,
}

#[derive(Deserialize)]
pub(super) struct CompanyOrderDto {
    pub(super) number: i64,
    pub(super) status: String,
    pub(super) total: String,
    pub(super) at: String,
}

#[derive(Deserialize)]
pub(super) struct CompanyDetailDto {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) subdomain: String,
    #[serde(default)]
    pub(super) domain: String,
    #[serde(default)]
    pub(super) logo: String,
    pub(super) created_at: String,
    pub(super) active: bool,
    pub(super) document: String,
    pub(super) phone: String,
    pub(super) whatsapp: String,
    pub(super) email: String,
    pub(super) address: String,
    pub(super) city_uf: String,
    pub(super) plan: String,
    pub(super) plan_amount: String,
    pub(super) status: String,
    pub(super) next_charge: String,
    pub(super) discount: String,
    pub(super) payment_method: String,
    pub(super) invoices_total: i64,
    pub(super) invoices_pending: i64,
    pub(super) orders_count: i64,
    pub(super) products_count: i64,
    pub(super) customers_count: i64,
    pub(super) last_order_at: String,
}

#[derive(Deserialize, Clone)]
pub(super) struct AdminDto {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) email: String,
    #[serde(default)]
    pub(super) role_id: String,
    #[serde(default)]
    pub(super) role_name: String,
    #[serde(default)]
    pub(super) avatar: String,
    #[serde(default = "default_true")]
    pub(super) active: bool,
}

fn default_true() -> bool {
    true
}

/// Função de administrador vinda de GET /admin/roles.
#[derive(Deserialize, Clone)]
pub(super) struct AdminRoleDto {
    pub(super) id: String,
    pub(super) name: String,
    #[serde(default)]
    pub(super) screens: Vec<String>,
    #[serde(default)]
    pub(super) users: i64,
}

#[derive(Deserialize, Clone, Default)]
pub(super) struct PlanDto {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) amount: f64,
    pub(super) period_days: i32,
    pub(super) trial_days: i32,
    pub(super) description: String,
    pub(super) highlight_label: String,
    pub(super) active: bool,
    pub(super) monthly_price: f64,
    /// Quantas empresas usam este plano (0 para payloads sem o campo).
    #[serde(default)]
    pub(super) companies: i64,
}

/// Tipo de empresa (ramo do estabelecimento) do catálogo do super admin.
#[derive(Deserialize, Clone, Default)]
pub(super) struct BusinessTypeDto {
    pub(super) id: String,
    pub(super) name: String,
    #[serde(default)]
    pub(super) description: String,
    pub(super) active: bool,
    #[serde(default)]
    pub(super) sort_order: i32,
}

/// Resposta de POST /admin/companies/{id}/impersonate (mesmo formato do
/// login-desktop). Reaproveita o fluxo `apply_login`/`update_ui_after_login`.
#[derive(Deserialize)]
pub(super) struct ImpersonateDto {
    pub(super) token: String,
    pub(super) user: ImpersonateUser,
    pub(super) subdomain: String,
    pub(super) company_name: String,
    #[serde(default)]
    pub(super) perms: Vec<String>,
}

#[derive(Deserialize)]
pub(super) struct ImpersonateUser {
    pub(super) company_id: Uuid,
    #[serde(default)]
    pub(super) name: String,
}
