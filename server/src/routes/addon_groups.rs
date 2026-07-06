use axum::extract::{Path, State};
use rust_decimal::Decimal;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use letaf_core::addon::model::Addon;
use letaf_core::addon_group::model::AddonGroup;

use crate::context::AppState;
use crate::error::ServerError;
use crate::jwt::ROLES_OPERATORS;
use crate::middleware::auth::AuthClaims;
use crate::middleware::tenant::TenantContext;

/// Rotas CRUD de grupos de adicionais e seus itens (Fase 4).
///
/// Regras aplicadas (AI_RULES.md §1, §11, §12):
/// - Handlers delegam ao service (sem lógica de domínio).
/// - JWT obrigatório (AuthClaims) — apenas operadores/admin.
/// - Isolamento por company_id (TenantContext).
/// - Respostas em JSON.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/addon-groups", get(list_groups).post(create_group))
        .route("/addon-groups/{id}", get(get_group).put(update_group).delete(delete_group))
        .route("/addon-groups/{id}/addons", get(list_addons).post(create_addon))
        .route("/addons/{id}", get(get_addon).put(update_addon).delete(delete_addon))
}

#[derive(Deserialize)]
struct GroupBody {
    name: String,
    selection: String,
    #[serde(default)]
    min_select: i32,
    #[serde(default)]
    max_select: i32,
}

#[derive(Deserialize)]
struct AddonBody {
    name: String,
    #[serde(default)]
    price: Decimal,
}

async fn list_groups(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
) -> Result<Json<Vec<AddonGroup>>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("addons.view")?;
    let items = state.addon_group_service.find_all(tenant.company_id).await?;
    Ok(Json(items))
}

async fn get_group(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
) -> Result<Json<AddonGroup>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("addons.view")?;
    let item = state.addon_group_service.find_by_id(tenant.company_id, id).await?
        .ok_or_else(|| ServerError::Core(letaf_core::error::CoreError::NotFound("AddonGroup not found".into())))?;
    Ok(Json(item))
}

async fn create_group(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Json(body): Json<GroupBody>,
) -> Result<(StatusCode, Json<AddonGroup>), ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("addons.edit")?;
    let g = state.addon_group_service
        .create(tenant.company_id, body.name, body.selection, body.min_select, body.max_select)
        .await?;
    Ok((StatusCode::CREATED, Json(g)))
}

async fn update_group(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
    Json(body): Json<GroupBody>,
) -> Result<Json<AddonGroup>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("addons.edit")?;
    let g = state.addon_group_service
        .update(tenant.company_id, id, body.name, body.selection, body.min_select, body.max_select)
        .await?;
    Ok(Json(g))
}

async fn delete_group(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("addons.edit")?;
    state.addon_group_service.soft_delete(tenant.company_id, id).await?;
    Ok(Json(json!({ "deleted": true })))
}

async fn list_addons(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(group_id): Path<Uuid>,
) -> Result<Json<Vec<Addon>>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("addons.view")?;
    let items = state.addon_service.find_by_group(tenant.company_id, group_id).await?;
    Ok(Json(items))
}

async fn create_addon(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(group_id): Path<Uuid>,
    Json(body): Json<AddonBody>,
) -> Result<(StatusCode, Json<Addon>), ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("addons.edit")?;
    let a = state.addon_service
        .create(tenant.company_id, group_id, body.name, body.price)
        .await?;
    Ok((StatusCode::CREATED, Json(a)))
}

#[derive(Deserialize)]
struct AddonUpdateBody {
    group_id: Uuid,
    name: String,
    #[serde(default)]
    price: Decimal,
}

async fn get_addon(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
) -> Result<Json<Addon>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("addons.view")?;
    let a = state.addon_service.find_by_id(tenant.company_id, id).await?
        .ok_or_else(|| ServerError::Core(letaf_core::error::CoreError::NotFound("Addon not found".into())))?;
    Ok(Json(a))
}

async fn update_addon(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
    Json(body): Json<AddonUpdateBody>,
) -> Result<Json<Addon>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("addons.edit")?;
    let a = state.addon_service
        .update(tenant.company_id, id, body.group_id, body.name, body.price)
        .await?;
    Ok(Json(a))
}

async fn delete_addon(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("addons.edit")?;
    state.addon_service.soft_delete(tenant.company_id, id).await?;
    Ok(Json(json!({ "deleted": true })))
}
