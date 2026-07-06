use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{routing::get, Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use letaf_core::payment_method::model::PaymentMethod;

use crate::context::AppState;
use crate::error::ServerError;
use crate::jwt::ROLES_OPERATORS;
use crate::middleware::auth::AuthClaims;
use crate::middleware::tenant::TenantContext;

/// Rotas REST de formas de pagamento.
///
/// Regras aplicadas (AI_RULES.md §1, §11, §12):
/// - Handler só converte HTTP ↔ domínio; lógica vive no service.
/// - JWT obrigatório + isolamento por `company_id`.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/payment-methods", get(list).post(create))
        .route(
            "/payment-methods/{id}",
            get(get_one).put(update).delete(delete),
        )
        .route(
            "/payment-methods/{id}/default",
            axum::routing::put(set_default),
        )
}

#[derive(Deserialize)]
struct CreateRequest {
    kind: String,
    label: String,
    #[serde(default)]
    masked: String,
    #[serde(default)]
    expiry: String,
    #[serde(default)]
    make_default: bool,
}

#[derive(Deserialize)]
struct UpdateRequest {
    label: String,
    #[serde(default)]
    masked: String,
    #[serde(default)]
    expiry: String,
    #[serde(default)]
    make_default: bool,
}

async fn list(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
) -> Result<Json<Vec<PaymentMethod>>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("finance.view")?;
    let items = state
        .payment_method_service
        .find_all(tenant.company_id)
        .await?;
    Ok(Json(items))
}

async fn get_one(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
) -> Result<Json<PaymentMethod>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("finance.view")?;
    let item = state
        .payment_method_service
        .find_by_id(tenant.company_id, id)
        .await?
        .ok_or_else(|| {
            ServerError::Core(letaf_core::error::CoreError::NotFound(
                "Forma de pagamento não encontrada".into(),
            ))
        })?;
    Ok(Json(item))
}

async fn create(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Json(body): Json<CreateRequest>,
) -> Result<(StatusCode, Json<PaymentMethod>), ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("finance.edit")?;
    let item = state
        .payment_method_service
        .create(
            tenant.company_id,
            body.kind,
            body.label,
            body.masked,
            body.expiry,
            body.make_default,
        )
        .await?;
    Ok((StatusCode::CREATED, Json(item)))
}

async fn update(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateRequest>,
) -> Result<Json<PaymentMethod>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("finance.edit")?;
    let item = state
        .payment_method_service
        .update(
            tenant.company_id,
            id,
            body.label,
            body.masked,
            body.expiry,
            body.make_default,
        )
        .await?;
    Ok(Json(item))
}

async fn set_default(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
) -> Result<Json<PaymentMethod>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("finance.edit")?;
    let item = state
        .payment_method_service
        .set_default(tenant.company_id, id)
        .await?;
    Ok(Json(item))
}

async fn delete(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("finance.edit")?;
    state
        .payment_method_service
        .delete(tenant.company_id, id)
        .await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}
