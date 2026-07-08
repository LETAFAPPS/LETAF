use axum::extract::{Query, State};
use axum::Json;
use serde_json::{json, Value};

use letaf_core::business_hours::model::BusinessHours;
use letaf_core::payment_method::model::PaymentMethod;
use letaf_core::subscription::model::{Invoice as SubscriptionInvoice, Subscription};

use crate::context::AppState;
use crate::error::ServerError;
use crate::jwt::ROLES_OPERATORS;
use crate::middleware::auth::AuthClaims;

use super::PullQuery;

pub(crate) async fn sync_payment_method(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(method): Json<PaymentMethod>,
) -> Result<Json<Value>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    auth.require_permission("finance.edit")?;
    state
        .payment_method_service
        .sync_upsert(auth.0.company_id, method)
        .await?;
    Ok(Json(json!({ "synced": true })))
}

pub(crate) async fn pull_payment_methods(
    State(state): State<AppState>,
    auth: AuthClaims,
    Query(params): Query<PullQuery>,
) -> Result<Json<Vec<PaymentMethod>>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    let items = state
        .payment_method_service
        .find_updated_since(auth.0.company_id, params.since)
        .await?;
    Ok(Json(items))
}

/// POST /sync/subscriptions — upsert da assinatura (desktop → server).
pub(crate) async fn sync_subscription(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(s): Json<Subscription>,
) -> Result<Json<Value>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    auth.require_permission("subscription.edit")?;
    state
        .subscription_service
        .sync_upsert_subscription(auth.0.company_id, s)
        .await?;
    Ok(Json(json!({ "synced": true })))
}

/// GET /sync/pull/subscriptions?since=<ts>
pub(crate) async fn pull_subscriptions(
    State(state): State<AppState>,
    auth: AuthClaims,
    Query(params): Query<PullQuery>,
) -> Result<Json<Vec<Subscription>>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    // Dado sensível (assinatura/billing): pull só para quem enxerga a tela.
    auth.require_permission("subscription.view")?;
    let items = state
        .subscription_service
        .find_subscriptions_updated_since(auth.0.company_id, params.since)
        .await?;
    Ok(Json(items))
}

/// POST /sync/subscription-invoices — upsert de fatura (desktop → server).
pub(crate) async fn sync_subscription_invoice(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(inv): Json<SubscriptionInvoice>,
) -> Result<Json<Value>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    auth.require_permission("subscription.edit")?;
    state
        .subscription_service
        .sync_upsert_invoice(auth.0.company_id, inv)
        .await?;
    Ok(Json(json!({ "synced": true })))
}

/// GET /sync/pull/subscription-invoices?since=<ts>
pub(crate) async fn pull_subscription_invoices(
    State(state): State<AppState>,
    auth: AuthClaims,
    Query(params): Query<PullQuery>,
) -> Result<Json<Vec<SubscriptionInvoice>>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    auth.require_permission("subscription.view")?;
    let items = state
        .subscription_service
        .find_invoices_updated_since_paged(
            auth.0.company_id,
            params.since,
            params.after_id(),
            params.page_limit(),
        )
        .await?;
    Ok(Json(items))
}

/// POST /sync/business-hours — upsert de horário sincronizado (desktop → servidor).
pub(crate) async fn sync_business_hours(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(bh): Json<BusinessHours>,
) -> Result<Json<Value>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    // Horário de funcionamento é config da vitrine (catálogo público).
    auth.require_permission("products.edit")?;
    state
        .business_hours_service
        .sync_upsert(auth.0.company_id, bh)
        .await?;
    Ok(Json(json!({ "synced": true })))
}

/// GET /sync/pull/business-hours?since=<timestamp> — pull de horários.
pub(crate) async fn pull_business_hours(
    State(state): State<AppState>,
    auth: AuthClaims,
    Query(params): Query<PullQuery>,
) -> Result<Json<Vec<BusinessHours>>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    let items = state
        .business_hours_service
        .find_updated_since(auth.0.company_id, params.since)
        .await?;
    Ok(Json(items))
}
