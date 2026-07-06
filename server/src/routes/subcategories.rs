use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use letaf_core::subcategory::model::Subcategory;

use crate::context::AppState;
use crate::error::ServerError;
use crate::jwt::ROLES_OPERATORS;
use crate::middleware::auth::AuthClaims;
use crate::middleware::tenant::TenantContext;

/// Rotas CRUD de subcategorias.
///
/// Regras aplicadas (AI_RULES.md §1, §11, §12):
/// - Handlers delegam ao service (sem lógica de domínio).
/// - JWT obrigatório (AuthClaims).
/// - Isolamento por company_id (TenantContext).
/// - Respostas em JSON.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/subcategories", get(list).post(create))
        .route("/subcategories/{id}", get(get_one).put(update).delete(delete))
}

#[derive(Deserialize)]
struct CreateSubcategoryRequest {
    category_id: Uuid,
    name: String,
}

#[derive(Deserialize)]
struct UpdateSubcategoryRequest {
    category_id: Uuid,
    name: String,
}

#[derive(Deserialize)]
struct ListFilter {
    category_id: Option<Uuid>,
}

/// GET /subcategories?category_id=<uuid>
async fn list(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Query(filter): Query<ListFilter>,
) -> Result<Json<Vec<Subcategory>>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("categories.view")?;
    let items = match filter.category_id {
        Some(cid) => state.subcategory_service.find_by_category(tenant.company_id, cid).await?,
        None => state.subcategory_service.find_all(tenant.company_id).await?,
    };
    Ok(Json(items))
}

async fn get_one(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
) -> Result<Json<Subcategory>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("categories.view")?;
    let item = state.subcategory_service.find_by_id(tenant.company_id, id).await?
        .ok_or_else(|| ServerError::Core(letaf_core::error::CoreError::NotFound("Subcategory not found".into())))?;
    Ok(Json(item))
}

async fn create(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Json(body): Json<CreateSubcategoryRequest>,
) -> Result<(StatusCode, Json<Subcategory>), ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("categories.edit")?;
    let item = state.subcategory_service
        .create(tenant.company_id, body.category_id, body.name)
        .await?;
    Ok((StatusCode::CREATED, Json(item)))
}

async fn update(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateSubcategoryRequest>,
) -> Result<Json<Subcategory>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("categories.edit")?;
    let item = state.subcategory_service
        .update(tenant.company_id, id, body.category_id, body.name)
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
    auth.require_permission("categories.edit")?;
    state.subcategory_service.soft_delete(tenant.company_id, id).await?;
    Ok(Json(json!({ "deleted": true })))
}
