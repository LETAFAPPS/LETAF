use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{routing::get, Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use letaf_core::customer::model::Customer;
use letaf_core::error::CoreError;

use crate::context::AppState;
use crate::error::ServerError;
use crate::jwt::ROLES_OPERATORS;
use crate::middleware::auth::AuthClaims;
use crate::middleware::tenant::TenantContext;

/// Rotas REST para Customer (protegidas por JWT).
///
/// Regras aplicadas (AI_RULES.md §1, §11, §12):
/// - GET → leitura
/// - POST → criação (201 Created)
/// - PUT → atualização
/// - DELETE → remoção lógica
/// - Respostas sempre em JSON
/// - Handler apenas converte HTTP ↔ domínio, sem lógica de negócio
/// - Autenticação obrigatória via JWT
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/customers", get(list).post(create))
        .route("/customers/{id}", get(get_one).put(update).delete(delete))
}

#[derive(Deserialize)]
struct CreateCustomerRequest {
    name: String,
    email: Option<String>,
    phone: Option<String>,
    document: Option<String>,
    #[serde(default)]
    notes: Option<String>,
}

#[derive(Deserialize)]
struct UpdateCustomerRequest {
    name: String,
    email: Option<String>,
    phone: Option<String>,
    document: Option<String>,
    #[serde(default)]
    notes: Option<String>,
}

/// GET /customers — lista todos os clientes da empresa.
async fn list(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
) -> Result<Json<Vec<Customer>>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("customers.view")?;
    let items = state.customer_service.find_all(tenant.company_id).await?;
    Ok(Json(items))
}

/// GET /customers/:id — busca cliente por ID.
async fn get_one(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
) -> Result<Json<Customer>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("customers.view")?;
    let item = state
        .customer_service
        .find_by_id(tenant.company_id, id)
        .await?
        .ok_or_else(|| ServerError::Core(CoreError::NotFound("Customer not found".into())))?;

    Ok(Json(item))
}

/// POST /customers — cria um novo cliente (201 Created).
async fn create(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Json(body): Json<CreateCustomerRequest>,
) -> Result<(StatusCode, Json<Customer>), ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("customers.edit")?;
    let item = state
        .customer_service
        .create(tenant.company_id, body.name, body.email, body.phone, body.document, body.notes)
        .await?;
    Ok((StatusCode::CREATED, Json(item)))
}

/// PUT /customers/:id — atualiza um cliente existente.
async fn update(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateCustomerRequest>,
) -> Result<Json<Customer>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("customers.edit")?;
    let item = state
        .customer_service
        .update(tenant.company_id, id, body.name, body.email, body.phone, body.document, body.notes)
        .await?;
    Ok(Json(item))
}

/// DELETE /customers/:id — remoção lógica (soft delete).
async fn delete(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("customers.edit")?;
    state
        .customer_service
        .soft_delete(tenant.company_id, id)
        .await?;

    Ok(Json(serde_json::json!({ "deleted": true })))
}
