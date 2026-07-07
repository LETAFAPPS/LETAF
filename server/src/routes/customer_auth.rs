use axum::extract::State;
use axum::http::StatusCode;
use axum::{routing::{get, post}, Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::context::AppState;
use crate::error::ServerError;
use crate::rate_limit::ClientIp;
use crate::jwt::{create_token, ROLE_CUSTOMER};
use crate::middleware::auth::AuthClaims;
use crate::middleware::tenant::TenantContext;

/// Rotas de autenticação para clientes finais (web/cardápio).
///
/// Regras aplicadas (AI_RULES.md §3, §5 Web, §11):
/// - Empresa identificada pelo subdomínio
/// - Cliente se registra/loga para fazer pedidos
/// - JWT emitido com customer_id (sub) + company_id
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/customer/register", post(register))
        .route("/customer/login", post(login))
        .route("/customer/profile", get(get_profile).put(update_profile))
}

#[derive(Deserialize)]
struct RegisterRequest {
    name: String,
    email: String,
    phone: Option<String>,
    password: String,
}

#[derive(Deserialize)]
struct LoginRequest {
    email: String,
    password: String,
}

#[derive(Serialize)]
struct CustomerAuthResponse {
    token: String,
    customer_id: Uuid,
    name: String,
}

#[derive(Serialize)]
struct CustomerProfileResponse {
    name: String,
    email: String,
    phone: Option<String>,
    profile_picture: Option<String>,
}

#[derive(Deserialize)]
struct UpdateProfileRequest {
    name: String,
    phone: Option<String>,
    password: Option<String>,
    current_password: Option<String>,
    profile_picture: Option<String>,
}

/// POST /customer/register — registra cliente final e retorna JWT.
async fn register(
    State(state): State<AppState>,
    tenant: TenantContext,
    Json(body): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<CustomerAuthResponse>), ServerError> {
    let customer = state
        .customer_service
        .register(
            tenant.company_id,
            body.name,
            body.email,
            body.phone,
            body.password,
        )
        .await?;

    let token = create_token(
        customer.base.id,
        tenant.company_id,
        ROLE_CUSTOMER,
        Vec::new(), // cliente final não tem permissões de operador
        0,          // cliente não versiona credencial (revogação é só p/ operador)
        &state.config.jwt_secret,
        72,
    )?;

    Ok((
        StatusCode::CREATED,
        Json(CustomerAuthResponse {
            token,
            customer_id: customer.base.id,
            name: customer.name,
        }),
    ))
}

/// GET /customer/profile — retorna dados do perfil do cliente autenticado.
async fn get_profile(
    State(state): State<AppState>,
    tenant: TenantContext,
    claims: AuthClaims,
) -> Result<Json<CustomerProfileResponse>, ServerError> {
    claims.verify(tenant.company_id, ROLE_CUSTOMER)?;
    let customer = state.customer_service
        .find_by_id(tenant.company_id, claims.0.sub)
        .await?
        .ok_or_else(|| letaf_core::error::CoreError::NotFound("Customer not found".into()))?;
    Ok(Json(CustomerProfileResponse {
        name:           customer.name,
        email:          customer.email.unwrap_or_default(),
        phone:          customer.phone,
        profile_picture: customer.profile_picture,
    }))
}

/// PUT /customer/profile — atualiza nome, telefone e senha do cliente autenticado.
async fn update_profile(
    State(state): State<AppState>,
    tenant: TenantContext,
    claims: AuthClaims,
    Json(body): Json<UpdateProfileRequest>,
) -> Result<Json<CustomerProfileResponse>, ServerError> {
    claims.verify(tenant.company_id, ROLE_CUSTOMER)?;
    let customer = state.customer_service
        .update_web_profile(
            tenant.company_id,
            claims.0.sub,
            body.name,
            body.phone,
            body.password,
            body.current_password,
            body.profile_picture,
        )
        .await?;
    Ok(Json(CustomerProfileResponse {
        name:            customer.name,
        email:           customer.email.unwrap_or_default(),
        phone:           customer.phone,
        profile_picture: customer.profile_picture,
    }))
}

/// POST /customer/login — autentica cliente final e retorna JWT.
async fn login(
    State(state): State<AppState>,
    tenant: TenantContext,
    ip: ClientIp,
    Json(body): Json<LoginRequest>,
) -> Result<Json<CustomerAuthResponse>, ServerError> {
    if !state.login_rate_limiter.check(ip.0) {
        return Err(ServerError::TooManyRequests(
            "Muitas tentativas. Aguarde alguns instantes e tente novamente.",
        ));
    }
    let customer = state
        .customer_service
        .authenticate(tenant.company_id, &body.email, &body.password)
        .await?;

    let token = create_token(
        customer.base.id,
        tenant.company_id,
        ROLE_CUSTOMER,
        Vec::new(), // cliente final não tem permissões de operador
        0,          // cliente não versiona credencial (revogação é só p/ operador)
        &state.config.jwt_secret,
        72,
    )?;

    Ok(Json(CustomerAuthResponse {
        token,
        customer_id: customer.base.id,
        name: customer.name,
    }))
}
