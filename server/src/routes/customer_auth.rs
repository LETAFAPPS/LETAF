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
        .route("/customer/forgot-password", post(forgot_password))
        .route("/customer/verify-reset-code", post(verify_reset_code))
        .route("/customer/reset-password", post(reset_password))
        .route("/customer/profile", get(get_profile).put(update_profile))
}

/// Mensagem única de excesso de tentativas (freia brute-force do código).
const RATE_LIMIT_MSG: &str = "Muitas tentativas. Aguarde alguns instantes e tente novamente.";

#[derive(Deserialize)]
struct RegisterRequest {
    name: String,
    email: String,
    phone: Option<String>,
    password: String,
}

#[derive(Deserialize)]
struct LoginRequest {
    /// E-mail ou telefone. `alias = "email"` mantém compatibilidade com
    /// clientes que ainda enviam o campo antigo.
    #[serde(alias = "email")]
    identifier: String,
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
    ip: ClientIp,
    Json(body): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<CustomerAuthResponse>), ServerError> {
    // Rate limit: evita criação em massa de contas de cliente (§11).
    if !state.login_rate_limiter.check(ip.0) {
        return Err(ServerError::TooManyRequests(
            "Muitas tentativas. Aguarde alguns instantes e tente novamente.",
        ));
    }
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
        0,          // cliente recém-criado → versão de credencial inicial 0
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

#[derive(Deserialize)]
struct ForgotPasswordRequest {
    email: String,
}

/// POST /customer/forgot-password — envia um código de recuperação por e-mail.
///
/// Responde SEMPRE 200 (anti-enumeração, §11) — só emite/envia quando há um
/// cliente ATIVO com aquele e-mail NESTA empresa (isolamento multi-tenant).
async fn forgot_password(
    State(state): State<AppState>,
    tenant: TenantContext,
    ip: ClientIp,
    Json(body): Json<ForgotPasswordRequest>,
) -> Result<StatusCode, ServerError> {
    if !state.login_rate_limiter.check(ip.0) {
        return Err(ServerError::TooManyRequests(RATE_LIMIT_MSG));
    }
    let email = body.email.trim().to_string();
    if let Ok(Some(_)) = state
        .customer_service
        .find_by_email(tenant.company_id, &email)
        .await
    {
        match state.password_reset_service.issue_code(&email).await {
            Ok(code) => {
                if let Err(e) =
                    crate::email::send_reset_code(&state.config.smtp, &email, &code).await
                {
                    tracing::error!("Falha ao enviar e-mail de recuperação (cliente): {e}");
                }
            }
            Err(e) => tracing::error!("Falha ao emitir código de recuperação (cliente): {e}"),
        }
    }
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
struct VerifyResetCodeRequest {
    email: String,
    code: String,
}

/// POST /customer/verify-reset-code — valida o código SEM consumir (libera a
/// tela de nova senha). A troca final revalida e consome (§11).
async fn verify_reset_code(
    State(state): State<AppState>,
    ip: ClientIp,
    Json(body): Json<VerifyResetCodeRequest>,
) -> Result<StatusCode, ServerError> {
    if !state.login_rate_limiter.check(ip.0) {
        return Err(ServerError::TooManyRequests(RATE_LIMIT_MSG));
    }
    state
        .password_reset_service
        .verify_code(body.email.trim(), body.code.trim())
        .await?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
struct ResetPasswordRequest {
    email: String,
    code: String,
    new_password: String,
}

/// POST /customer/reset-password — conclui a recuperação: consome o código e
/// troca a senha do cliente (escopo do tenant).
async fn reset_password(
    State(state): State<AppState>,
    tenant: TenantContext,
    ip: ClientIp,
    Json(body): Json<ResetPasswordRequest>,
) -> Result<StatusCode, ServerError> {
    if !state.login_rate_limiter.check(ip.0) {
        return Err(ServerError::TooManyRequests(RATE_LIMIT_MSG));
    }
    let email = body.email.trim().to_string();
    state
        .password_reset_service
        .verify_and_consume(&email, body.code.trim())
        .await?;
    state
        .customer_service
        .reset_password_by_email(tenant.company_id, &email, &body.new_password)
        .await?;
    Ok(StatusCode::OK)
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
        .authenticate_by_identifier(tenant.company_id, &body.identifier, &body.password)
        .await?;

    // Versão de credencial atual (§11): carimbada no token para que trocar a
    // senha / banir o cliente invalide sessões antigas no próximo request.
    let tv = state
        .customer_service
        .find_token_version(tenant.company_id, customer.base.id)
        .await?
        .unwrap_or(0);
    let token = create_token(
        customer.base.id,
        tenant.company_id,
        ROLE_CUSTOMER,
        Vec::new(), // cliente final não tem permissões de operador
        tv,
        &state.config.jwt_secret,
        72,
    )?;

    Ok(Json(CustomerAuthResponse {
        token,
        customer_id: customer.base.id,
        name: customer.name,
    }))
}
