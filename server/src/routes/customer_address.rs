use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{routing::{get, put}, Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use letaf_core::customer_address::model::CustomerAddress;

use crate::context::AppState;
use crate::error::ServerError;
use crate::jwt::ROLE_CUSTOMER;
use crate::middleware::auth::AuthClaims;
use crate::middleware::tenant::TenantContext;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/customer/addresses", get(list_addresses).post(create_address))
        .route("/customer/addresses/{id}", put(update_address).delete(delete_address))
}

#[derive(Serialize)]
struct AddressResponse {
    id: String,
    label: String,
    custom_label: Option<String>,
    street: String,
    number: String,
    neighborhood: String,
    apartment: Option<String>,
}

impl From<CustomerAddress> for AddressResponse {
    fn from(a: CustomerAddress) -> Self {
        Self {
            id:           a.base.id.to_string(),
            label:        a.label,
            custom_label: a.custom_label,
            street:       a.street,
            number:       a.number,
            neighborhood: a.neighborhood,
            apartment:    a.apartment,
        }
    }
}

#[derive(Deserialize)]
struct CreateAddressRequest {
    label: String,
    custom_label: Option<String>,
    street: String,
    number: String,
    neighborhood: String,
    apartment: Option<String>,
}

/// GET /customer/addresses — lista endereços do cliente autenticado.
async fn list_addresses(
    State(state): State<AppState>,
    tenant: TenantContext,
    claims: AuthClaims,
) -> Result<Json<Vec<AddressResponse>>, ServerError> {
    claims.verify(tenant.company_id, ROLE_CUSTOMER)?;
    let list = state.customer_address_service
        .list(tenant.company_id, claims.0.sub)
        .await?;
    Ok(Json(list.into_iter().map(AddressResponse::from).collect()))
}

/// POST /customer/addresses — cria novo endereço para o cliente.
async fn create_address(
    State(state): State<AppState>,
    tenant: TenantContext,
    claims: AuthClaims,
    Json(body): Json<CreateAddressRequest>,
) -> Result<(StatusCode, Json<AddressResponse>), ServerError> {
    claims.verify(tenant.company_id, ROLE_CUSTOMER)?;
    let addr = state.customer_address_service
        .create(
            tenant.company_id,
            claims.0.sub,
            body.label,
            body.custom_label,
            body.street,
            body.number,
            body.neighborhood,
            body.apartment,
        )
        .await?;
    Ok((StatusCode::CREATED, Json(AddressResponse::from(addr))))
}

/// PUT /customer/addresses/{id} — atualiza endereço do cliente.
async fn update_address(
    State(state): State<AppState>,
    tenant: TenantContext,
    claims: AuthClaims,
    Path(id): Path<Uuid>,
    Json(body): Json<CreateAddressRequest>,
) -> Result<Json<AddressResponse>, ServerError> {
    claims.verify(tenant.company_id, ROLE_CUSTOMER)?;
    let addr = state.customer_address_service
        .update(
            tenant.company_id,
            id,
            claims.0.sub,
            body.label,
            body.custom_label,
            body.street,
            body.number,
            body.neighborhood,
            body.apartment,
        )
        .await?;
    Ok(Json(AddressResponse::from(addr)))
}

/// DELETE /customer/addresses/:id — remove endereço logicamente.
async fn delete_address(
    State(state): State<AppState>,
    tenant: TenantContext,
    claims: AuthClaims,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ServerError> {
    claims.verify(tenant.company_id, ROLE_CUSTOMER)?;
    state.customer_address_service
        .soft_delete(tenant.company_id, id, claims.0.sub)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
