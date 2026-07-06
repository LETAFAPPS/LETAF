use axum::extract::{Path, State};
use rust_decimal::Decimal;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use chrono::NaiveDateTime;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use letaf_core::coupon::model::Coupon;

use crate::context::AppState;
use crate::error::ServerError;
use crate::jwt::ROLES_OPERATORS;
use crate::middleware::auth::AuthClaims;
use crate::middleware::tenant::TenantContext;

/// Rotas CRUD de cupons (Fase 8).
///
/// Regras aplicadas (AI_RULES.md §1, §11, §12):
/// - Handlers só orquestram; validação/regra fica no service do core.
/// - JWT obrigatório + isolamento multi-tenant (TenantContext).
/// - `set_active` dedicado para ativar/desativar sem reenviar o resto.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/coupons", get(list).post(create))
        .route("/coupons/{id}", get(get_one).put(update).delete(delete))
        .route("/coupons/{id}/active", axum::routing::patch(set_active))
}

#[derive(Deserialize)]
struct CouponBody {
    title: String,
    code: String,
    coupon_type: String,
    discount_kind: String,
    #[serde(default)]
    discount_value: Decimal,
    #[serde(default)]
    min_order_value: Decimal,
    #[serde(default)]
    max_discount: Decimal,
    #[serde(default)]
    per_user_limit: i32,
    #[serde(default)]
    usage_limit: i32,
    #[serde(default)]
    valid_from: Option<NaiveDateTime>,
    #[serde(default)]
    valid_until: Option<NaiveDateTime>,
}

#[derive(Deserialize)]
struct SetActiveRequest {
    active: bool,
}

async fn list(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
) -> Result<Json<Vec<Coupon>>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("coupons.view")?;
    let items = state.coupon_service.find_all(tenant.company_id).await?;
    Ok(Json(items))
}

async fn get_one(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
) -> Result<Json<Coupon>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("coupons.view")?;
    let item = state.coupon_service.find_by_id(tenant.company_id, id).await?
        .ok_or_else(|| ServerError::Core(letaf_core::error::CoreError::NotFound("Coupon not found".into())))?;
    Ok(Json(item))
}

async fn create(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Json(b): Json<CouponBody>,
) -> Result<(StatusCode, Json<Coupon>), ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("coupons.edit")?;
    let item = state.coupon_service
        .create(tenant.company_id, b.title, b.code, b.coupon_type, b.discount_kind,
                b.discount_value, b.min_order_value, b.max_discount,
                b.per_user_limit, b.usage_limit, b.valid_from, b.valid_until)
        .await?;
    Ok((StatusCode::CREATED, Json(item)))
}

async fn update(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
    Json(b): Json<CouponBody>,
) -> Result<Json<Coupon>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("coupons.edit")?;
    let item = state.coupon_service
        .update(tenant.company_id, id, b.title, b.code, b.coupon_type, b.discount_kind,
                b.discount_value, b.min_order_value, b.max_discount,
                b.per_user_limit, b.usage_limit, b.valid_from, b.valid_until)
        .await?;
    Ok(Json(item))
}

async fn delete(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("coupons.edit")?;
    state.coupon_service.soft_delete(tenant.company_id, id).await?;
    Ok(Json(json!({ "deleted": true })))
}

async fn set_active(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
    Json(body): Json<SetActiveRequest>,
) -> Result<Json<Value>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("coupons.edit")?;
    state.coupon_service.set_active(tenant.company_id, id, body.active).await?;
    Ok(Json(json!({ "active": body.active })))
}
