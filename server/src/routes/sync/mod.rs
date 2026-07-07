//! Rotas de sincronização bidirecional (offline-first, AI_RULES §7/§11/§12).
//!
//! Handlers agrupados por domínio (§8): `catalog`, `customers`, `orders`,
//! `finance`, `billing`. Push (POST, synced=false → upsert) e pull (GET,
//! `?since=` → atualizados desde o timestamp), com last-write-wins por
//! `updated_at` e validação de `company_id` contra o JWT.

use axum::{routing::{get, post}, Router};
use chrono::NaiveDateTime;
use serde::Deserialize;
use uuid::Uuid;

use crate::context::AppState;

mod billing;
mod catalog;
mod customers;
mod finance;
mod orders;

use billing::*;
use catalog::*;
use customers::*;
use finance::*;
use orders::*;

/// Parâmetro de query para pull: `?since=2024-01-01T00:00:00`. Para endpoints
/// paginados, o cliente também envia `&after_id=<uuid>&limit=<n>` (keyset).
#[derive(Deserialize)]
pub(crate) struct PullQuery {
    #[serde(default = "default_since")]
    pub(crate) since: NaiveDateTime,
    /// Componente `id` do cursor keyset (desempate de `updated_at` iguais).
    #[serde(default)]
    pub(crate) after_id: Option<Uuid>,
    /// Tamanho da página pedido pelo cliente (limitado por `MAX_PAGE_LIMIT`).
    #[serde(default)]
    pub(crate) limit: Option<i64>,
}

/// Teto de linhas por página do pull — protege o servidor de um `limit`
/// abusivo vindo do cliente.
const MAX_PAGE_LIMIT: i64 = 1000;

impl PullQuery {
    /// `after_id` do cursor (nil quando ausente = início).
    pub(crate) fn after_id(&self) -> Uuid {
        self.after_id.unwrap_or_else(Uuid::nil)
    }
    /// Tamanho de página efetivo, sempre em `[1, MAX_PAGE_LIMIT]`.
    pub(crate) fn page_limit(&self) -> i64 {
        self.limit.unwrap_or(MAX_PAGE_LIMIT).clamp(1, MAX_PAGE_LIMIT)
    }
}

fn default_since() -> NaiveDateTime {
    NaiveDateTime::default()
}

/// Registra todas as rotas de sincronização.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/sync/products", post(sync_product))
        .route("/sync/reconcile/manifest", get(reconcile_manifest))
        .route("/sync/stock-movements", post(sync_stock_movement))
        .route("/sync/pull/stock-movements", get(pull_stock_movements))
        .route("/sync/users", post(sync_user))
        .route("/sync/companies", post(sync_company))
        .route("/sync/customers", post(sync_customer))
        .route("/sync/pull/products", get(pull_products))
        .route("/sync/pull/users", get(pull_users))
        .route("/sync/pull/companies", get(pull_companies))
        .route("/sync/pull/customers", get(pull_customers))
        .route("/sync/categories", post(sync_category))
        .route("/sync/pull/categories", get(pull_categories))
        .route("/sync/job-roles", post(sync_job_role))
        .route("/sync/pull/job-roles", get(pull_job_roles))
        .route("/sync/subcategories", post(sync_subcategory))
        .route("/sync/pull/subcategories", get(pull_subcategories))
        .route("/sync/orders", post(sync_order))
        .route("/sync/pull/orders", get(pull_orders))
        .route("/sync/business-hours", post(sync_business_hours))
        .route("/sync/pull/business-hours", get(pull_business_hours))
        .route("/sync/addon-groups", post(sync_addon_group))
        .route("/sync/pull/addon-groups", get(pull_addon_groups))
        .route("/sync/addons", post(sync_addon))
        .route("/sync/pull/addons", get(pull_addons))
        .route("/sync/banners", post(sync_banner))
        .route("/sync/pull/banners", get(pull_banners))
        .route("/sync/coupons", post(sync_coupon))
        .route("/sync/pull/coupons", get(pull_coupons))
        .route("/sync/customer-addresses", post(sync_customer_address))
        .route("/sync/pull/customer-addresses", get(pull_customer_addresses))
        .route("/sync/cash-sessions", post(sync_cash_session))
        .route("/sync/pull/cash-sessions", get(pull_cash_sessions))
        .route("/sync/cash-movements", post(sync_cash_movement))
        .route("/sync/pull/cash-movements", get(pull_cash_movements))
        .route("/sync/finance-categories", post(sync_finance_category))
        .route("/sync/pull/finance-categories", get(pull_finance_categories))
        .route("/sync/finance-entries", post(sync_finance_entry))
        .route("/sync/pull/finance-entries", get(pull_finance_entries))
        .route("/sync/wallet-accounts", post(sync_wallet_account))
        .route("/sync/pull/wallet-accounts", get(pull_wallet_accounts))
        .route("/sync/wallet-movements", post(sync_wallet_movement))
        .route("/sync/pull/wallet-movements", get(pull_wallet_movements))
        .route("/sync/subscriptions", post(sync_subscription))
        .route("/sync/pull/subscriptions", get(pull_subscriptions))
        .route("/sync/subscription-invoices", post(sync_subscription_invoice))
        .route("/sync/pull/subscription-invoices", get(pull_subscription_invoices))
        .route("/sync/payment-methods", post(sync_payment_method))
        .route("/sync/pull/payment-methods", get(pull_payment_methods))
}
