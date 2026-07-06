use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use letaf_core::category::model::Category;

use crate::context::AppState;
use crate::error::ServerError;
use crate::jwt::ROLES_OPERATORS;
use crate::middleware::auth::AuthClaims;
use crate::middleware::tenant::TenantContext;

/// Rotas CRUD de categorias.
///
/// Regras aplicadas (AI_RULES.md §1, §11, §12):
/// - Handlers delegam ao service (sem lógica de domínio)
/// - JWT obrigatório (AuthClaims)
/// - Isolamento por company_id (TenantContext)
/// - Respostas em JSON
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/categories", get(list).post(create))
        .route("/categories/{id}", get(get_one).put(update).delete(delete))
}

#[derive(Deserialize)]
struct CreateCategoryRequest {
    name: String,
    description: Option<String>,
    #[serde(default)]
    icon_name: Option<String>,
}

#[derive(Deserialize)]
struct UpdateCategoryRequest {
    name: String,
    description: Option<String>,
    #[serde(default)]
    icon_name: Option<String>,
}

/// GET /categories
async fn list(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
) -> Result<Json<Vec<Category>>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("categories.view")?;
    let items = state.category_service.find_all(tenant.company_id).await?;
    Ok(Json(items))
}

/// GET /categories/:id
async fn get_one(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
) -> Result<Json<Category>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("categories.view")?;
    let item = state.category_service.find_by_id(tenant.company_id, id).await?
        .ok_or_else(|| ServerError::Core(letaf_core::error::CoreError::NotFound("Category not found".into())))?;
    Ok(Json(item))
}

/// POST /categories (201 Created)
async fn create(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Json(body): Json<CreateCategoryRequest>,
) -> Result<(StatusCode, Json<Category>), ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("categories.edit")?;
    let item = state.category_service
        .create(tenant.company_id, body.name, body.description, body.icon_name)
        .await?;
    Ok((StatusCode::CREATED, Json(item)))
}

/// PUT /categories/:id
async fn update(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateCategoryRequest>,
) -> Result<Json<Category>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("categories.edit")?;
    let item = state.category_service
        .update(tenant.company_id, id, body.name, body.description, body.icon_name)
        .await?;
    Ok(Json(item))
}

/// DELETE /categories/:id (soft delete)
async fn delete(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("categories.edit")?;
    state.category_service.soft_delete(tenant.company_id, id).await?;
    Ok(Json(json!({ "deleted": true })))
}
