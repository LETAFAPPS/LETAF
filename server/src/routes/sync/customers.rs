use axum::extract::{Query, State};
use axum::Json;
use serde_json::{json, Value};

use letaf_core::auth::model::SyncUserPayload;
use letaf_core::customer_address::model::CustomerAddress;
use letaf_core::company::model::Company;
use letaf_core::customer::model::Customer;

use crate::context::AppState;
use crate::error::ServerError;
use crate::jwt::{ROLE_ADMIN, ROLE_SUPER_ADMIN, ROLES_OPERATORS};
use crate::middleware::auth::AuthClaims;

use super::PullQuery;

/// POST /sync/users — upsert de usuário sincronizado.
///
/// §11: o `role` do payload NÃO é confiável. O serviço rejeita `super_admin`,
/// preserva o role de usuários existentes e só deixa um Admin introduzir um
/// novo Admin — por isso repassamos se o chamador é Admin (nunca o role bruto).
pub(crate) async fn sync_user(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(payload): Json<SyncUserPayload>,
) -> Result<Json<Value>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    let caller_is_admin = auth.0.role == ROLE_ADMIN || auth.0.role == ROLE_SUPER_ADMIN;
    state
        .auth_service
        .sync_upsert_from_client(auth.0.company_id, caller_is_admin, payload)
        .await?;

    Ok(Json(json!({ "synced": true })))
}

/// POST /sync/companies — upsert de empresa sincronizada.
pub(crate) async fn sync_company(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(company): Json<Company>,
) -> Result<Json<Value>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    state
        .company_service
        .sync_upsert(auth.0.company_id, company)
        .await?;

    Ok(Json(json!({ "synced": true })))
}

/// GET /sync/pull/users?since=<timestamp> — pull de usuários.
///
/// Retorna SyncUserPayload para incluir password_hash na replicação.
pub(crate) async fn pull_users(
    State(state): State<AppState>,
    auth: AuthClaims,
    Query(params): Query<PullQuery>,
) -> Result<Json<Vec<SyncUserPayload>>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    let users = state
        .auth_service
        .find_updated_since(auth.0.company_id, params.since)
        .await?;

    let payloads: Vec<SyncUserPayload> = users.iter().map(SyncUserPayload::from).collect();
    Ok(Json(payloads))
}

/// GET /sync/pull/companies?since=<timestamp> — pull de empresa.
pub(crate) async fn pull_companies(
    State(state): State<AppState>,
    auth: AuthClaims,
    Query(params): Query<PullQuery>,
) -> Result<Json<Vec<Company>>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    let items = state
        .company_service
        .find_updated_since(auth.0.company_id, params.since)
        .await?;

    Ok(Json(items))
}

/// POST /sync/customers — upsert de cliente sincronizado.
pub(crate) async fn sync_customer(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(customer): Json<Customer>,
) -> Result<Json<Value>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    state
        .customer_service
        .sync_upsert(auth.0.company_id, customer)
        .await?;

    Ok(Json(json!({ "synced": true })))
}

/// GET /sync/pull/customers?since=<timestamp> — pull de clientes.
pub(crate) async fn pull_customers(
    State(state): State<AppState>,
    auth: AuthClaims,
    Query(params): Query<PullQuery>,
) -> Result<Json<Vec<Customer>>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    let items = state
        .customer_service
        .find_updated_since_paged(
            auth.0.company_id,
            params.since,
            params.after_id(),
            params.page_limit(),
        )
        .await?;

    Ok(Json(items))
}

/// POST /sync/customer-addresses — upsert de endereço (desktop → servidor).
/// Endereços criados no balcão (desktop) e no app (web) compartilham a
/// mesma tabela; sync por last-write-wins (§7.7).
pub(crate) async fn sync_customer_address(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(address): Json<CustomerAddress>,
) -> Result<Json<Value>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    state.customer_address_service
        .sync_upsert(auth.0.company_id, address)
        .await?;
    Ok(Json(json!({ "synced": true })))
}

/// GET /sync/pull/customer-addresses?since=<timestamp>
pub(crate) async fn pull_customer_addresses(
    State(state): State<AppState>,
    auth: AuthClaims,
    Query(params): Query<PullQuery>,
) -> Result<Json<Vec<CustomerAddress>>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    let items = state.customer_address_service
        .find_updated_since(auth.0.company_id, params.since)
        .await?;
    Ok(Json(items))
}
