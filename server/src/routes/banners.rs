use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use letaf_core::banner::model::Banner;

use crate::context::AppState;
use crate::error::ServerError;
use crate::jwt::ROLES_OPERATORS;
use crate::middleware::auth::AuthClaims;
use crate::middleware::tenant::TenantContext;

/// Rotas CRUD de banners (Fase 7).
///
/// Regras aplicadas (AI_RULES.md §1, §11, §12):
/// - Handlers só orquestram; lógica fica no service.
/// - JWT obrigatório + isolamento multi-tenant.
/// - `set_active` é um endpoint dedicado para alternar visibilidade
///   sem precisar reenviar a imagem (que é base64 e pesa).
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/banners", get(list).post(create))
        .route("/banners/{id}", get(get_one).put(update).delete(delete))
        .route("/banners/{id}/active", axum::routing::patch(set_active))
}

#[derive(Deserialize)]
struct CreateBannerRequest {
    title: String,
    image_data: String,
    item_type: String,
    #[serde(default)]
    item_id: Option<Uuid>,
    #[serde(default)]
    item_url: Option<String>,
}

#[derive(Deserialize)]
struct UpdateBannerRequest {
    title: String,
    image_data: String,
    item_type: String,
    #[serde(default)]
    item_id: Option<Uuid>,
    #[serde(default)]
    item_url: Option<String>,
}

#[derive(Deserialize)]
struct SetActiveRequest {
    active: bool,
}

async fn list(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
) -> Result<Json<Vec<Banner>>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("banners.view")?;
    let items = state.banner_service.find_all(tenant.company_id).await?;
    Ok(Json(items))
}

async fn get_one(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
) -> Result<Json<Banner>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("banners.view")?;
    let item = state.banner_service.find_by_id(tenant.company_id, id).await?
        .ok_or_else(|| ServerError::Core(letaf_core::error::CoreError::NotFound("Banner not found".into())))?;
    Ok(Json(item))
}

async fn create(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Json(body): Json<CreateBannerRequest>,
) -> Result<(StatusCode, Json<Banner>), ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("banners.edit")?;
    let item = state.banner_service
        .create(tenant.company_id, body.title, body.image_data, body.item_type, body.item_id, body.item_url)
        .await?;
    Ok((StatusCode::CREATED, Json(item)))
}

async fn update(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateBannerRequest>,
) -> Result<Json<Banner>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("banners.edit")?;
    let item = state.banner_service
        .update(tenant.company_id, id, body.title, body.image_data, body.item_type, body.item_id, body.item_url)
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
    auth.require_permission("banners.edit")?;
    state.banner_service.soft_delete(tenant.company_id, id).await?;
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
    auth.require_permission("banners.edit")?;
    state.banner_service.set_active(tenant.company_id, id, body.active).await?;
    Ok(Json(json!({ "active": body.active })))
}
