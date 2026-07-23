use axum::extract::State;
use axum::http::StatusCode;
use axum::{routing::{get, post, put}, Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use letaf_core::auth::model::{User, UserRole};

use crate::context::AppState;
use crate::error::ServerError;
use crate::jwt::create_token;
use crate::middleware::auth::AuthClaims;
use crate::middleware::tenant::TenantContext;
use crate::rate_limit::ClientIp;

/// Mensagem de 429 nos endpoints de autenticação.
const RATE_LIMIT_MSG: &str = "Muitas tentativas. Aguarde alguns instantes e tente novamente.";

/// Rotas REST para autenticação.
///
/// Regras aplicadas (AI_RULES.md §1, §11, §12):
/// - POST /auth/register → criação (201 Created)
/// - POST /auth/login → autenticação (200 OK)
/// - Respostas sempre em JSON
/// - Handler apenas converte HTTP ↔ domínio, sem lógica de negócio
/// - Validação e hash de senha delegados ao AuthService
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/auth/login-desktop", post(login_desktop))
        .route("/auth/me", get(me))
        .route("/auth/profile", put(update_profile))
        .route("/auth/forgot-password", post(forgot_password))
        .route("/auth/verify-reset-code", post(verify_reset_code))
        .route("/auth/reset-password", post(reset_password))
}

/// PUT /auth/profile — o próprio operador edita suas credenciais
/// (nome, e-mail e, opcionalmente, senha). Alvo = usuário do JWT (`sub`).
///
/// Regras (§11): só operadores (admin/funcionário/super admin) — clientes
/// finais têm perfil próprio no web. O e-mail deve ser único no sistema
/// (login é global por e-mail), excluindo o próprio usuário.
#[derive(Deserialize)]
struct UpdateProfileRequest {
    name: String,
    email: String,
    /// Nova senha; vazia/ausente mantém a atual.
    #[serde(default)]
    password: Option<String>,
    /// Foto de perfil (base64). `Some("")` remove; ausente mantém a atual.
    #[serde(default)]
    avatar: Option<String>,
}

async fn update_profile(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(body): Json<UpdateProfileRequest>,
) -> Result<StatusCode, ServerError> {
    auth.verify_any_role(&[
        crate::jwt::ROLE_ADMIN,
        crate::jwt::ROLE_EMPLOYEE,
        crate::jwt::ROLE_SUPER_ADMIN,
    ])?;
    // Unicidade global do e-mail (exceto o próprio usuário).
    match state.auth_service.find_by_email_global(&body.email).await {
        Ok(None) => {}
        Ok(Some(u)) if u.base.id == auth.0.sub => {}
        _ => {
            return Err(ServerError::Core(letaf_core::error::CoreError::Validation(
                "Este e-mail já está em uso em outra conta do sistema.".into(),
            )))
        }
    }
    state
        .auth_service
        .update_credentials(
            auth.0.company_id,
            auth.0.sub,
            body.email,
            body.name,
            body.password,
            body.avatar,
        )
        .await?;
    Ok(StatusCode::OK)
}

/// POST /auth/forgot-password — inicia a recuperação de senha.
///
/// Regras (§11): responde SEMPRE 200, mesmo se o e-mail não existir, para
/// não vazar quais e-mails estão cadastrados. Só emite/envia o código
/// quando há um usuário único com aquele e-mail.
#[derive(Deserialize)]
struct ForgotPasswordRequest {
    email: String,
}

async fn forgot_password(
    State(state): State<AppState>,
    ip: ClientIp,
    Json(body): Json<ForgotPasswordRequest>,
) -> Result<StatusCode, ServerError> {
    if !state.login_rate_limiter.check(ip.0) {
        return Err(ServerError::TooManyRequests(RATE_LIMIT_MSG));
    }
    let email = body.email.trim().to_string();
    // `find_by_email_global` → Err se duplicado (ambíguo) ou None se não existe.
    if let Ok(Some(_)) = state.auth_service.find_by_email_global(&email).await {
        match state.password_reset_service.issue_code(&email).await {
            Ok(code) => {
                if let Err(e) =
                    crate::email::send_reset_code(&state.config.smtp, &email, &code).await
                {
                    tracing::error!("Falha ao enviar e-mail de recuperação p/ {email}: {e}");
                }
            }
            Err(e) => tracing::error!("Falha ao emitir código de recuperação: {e}"),
        }
    }
    Ok(StatusCode::OK)
}

/// POST /auth/verify-reset-code — valida o código SEM consumir, para
/// liberar a tela de nova senha só quando o código estiver correto.
///
/// Regras (§11): a autoridade é o backend; o frontend só avança de tela
/// se este endpoint responder 200. A troca final (`reset-password`)
/// revalida e consome o código.
#[derive(Deserialize)]
struct VerifyResetCodeRequest {
    email: String,
    code: String,
}

async fn verify_reset_code(
    State(state): State<AppState>,
    ip: ClientIp,
    Json(body): Json<VerifyResetCodeRequest>,
) -> Result<StatusCode, ServerError> {
    // Rate limit: freia brute-force do código de 6 dígitos (§11).
    if !state.login_rate_limiter.check(ip.0) {
        return Err(ServerError::TooManyRequests(RATE_LIMIT_MSG));
    }
    let email = body.email.trim().to_string();
    state
        .password_reset_service
        .verify_code(&email, body.code.trim())
        .await?;
    Ok(StatusCode::OK)
}

/// POST /auth/reset-password — conclui a recuperação com o código.
#[derive(Deserialize)]
struct ResetPasswordRequest {
    email: String,
    code: String,
    new_password: String,
}

async fn reset_password(
    State(state): State<AppState>,
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
        .auth_service
        .reset_password_global(&email, &body.new_password)
        .await?;
    Ok(StatusCode::OK)
}

#[derive(Serialize)]
struct MeResponse {
    user_id: Uuid,
    company_id: Uuid,
    role: String,
    name: String,
    email: String,
    /// Foto de perfil (base64) ou `null` se o operador não tiver foto.
    avatar: Option<String>,
}

/// GET /auth/me — valida token e retorna claims.
///
/// Regras aplicadas (AI_RULES.md §11):
/// - Token deve ser válido (assinatura + expiração)
/// - `company_id` do token deve corresponder a uma empresa existente
///   (protege contra tokens stale após reset de banco)
async fn me(
    State(state): State<AppState>,
    auth: AuthClaims,
) -> Result<Json<MeResponse>, ServerError> {
    let user = state
        .auth_service
        .find_by_id(auth.0.company_id, auth.0.sub)
        .await?
        .ok_or_else(|| ServerError::Core(letaf_core::error::CoreError::Unauthorized(
            "User not found for token".into(),
        )))?;

    Ok(Json(MeResponse {
        user_id: auth.0.sub,
        company_id: auth.0.company_id,
        role: auth.0.role.clone(),
        name: user.name,
        email: user.email,
        avatar: user.avatar,
    }))
}

#[derive(Deserialize)]
struct RegisterRequest {
    email: String,
    password: String,
    name: String,
}

#[derive(Deserialize)]
struct LoginRequest {
    email: String,
    password: String,
}

/// Payload minimalista do usuário em respostas de login/register.
///
/// Regras aplicadas (AI_RULES.md §11):
/// - Não expor `BaseFields` (synced, deleted_at, created_at, updated_at)
///   — são metadados internos sem valor para o cliente.
/// - `company_id` permanece porque o desktop precisa para configurar a
///   sessão local; em rotas que usam TenantContext do subdomínio o
///   cliente já conhece o tenant.
#[derive(Serialize)]
struct AuthUserPayload {
    id: uuid::Uuid,
    company_id: uuid::Uuid,
    name: String,
    email: String,
    role: letaf_core::auth::model::UserRole,
}

impl From<&User> for AuthUserPayload {
    fn from(u: &User) -> Self {
        Self {
            id: u.base.id,
            company_id: u.base.company_id,
            name: u.name.clone(),
            email: u.email.clone(),
            role: u.role,
        }
    }
}

#[derive(Serialize)]
struct AuthResponse {
    token: String,
    user: AuthUserPayload,
    /// Permissões efetivas (RBAC) — a UI usa para gatear funcionalidades.
    perms: Vec<String>,
}

#[derive(Serialize)]
struct DesktopAuthResponse {
    token: String,
    user: AuthUserPayload,
    subdomain: String,
    company_name: String,
    /// Permissões efetivas (RBAC) — o desktop esconde/desabilita as abas.
    perms: Vec<String>,
}

/// Resolve as permissões efetivas do operador (RBAC §11).
/// Admin/SuperAdmin têm acesso total (catálogo completo); Employee herda
/// as permissões da Função atribuída (vazio se sem função).
async fn resolve_perms(state: &AppState, user: &User) -> Vec<String> {
    match user.role {
        UserRole::Admin | UserRole::SuperAdmin => letaf_core::permission::all(),
        UserRole::Employee => match user.job_role_id {
            Some(jid) => state
                .job_role_service
                .find_by_id(user.base.company_id, jid)
                .await
                .ok()
                .flatten()
                .map(|jr| jr.permissions)
                .unwrap_or_default(),
            None => Vec::new(),
        },
    }
}

/// POST /auth/register — cria o usuário Admin INICIAL e retorna token JWT.
///
/// Regras aplicadas (AI_RULES.md §11 — nunca confiar no frontend,
/// nunca expor/alterar dados entre empresas):
/// - Cadastro público só é permitido para a PRIMEIRA conta da empresa
///   (dono se cadastrando). Se a empresa já tiver qualquer usuário,
///   a rota é recusada — caso contrário qualquer pessoa que conheça
///   o subdomínio poderia criar um Admin e assumir o tenant.
/// - Criação de usuários adicionais deve ser feita por um Admin
///   autenticado via endpoint protegido.
async fn register(
    State(state): State<AppState>,
    tenant: TenantContext,
    Json(body): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<AuthResponse>), ServerError> {
    // Bloqueia auto-cadastro se a empresa já possui usuários.
    let existing = state.auth_service.find_all(tenant.company_id).await?;
    if !existing.is_empty() {
        return Err(ServerError::Core(letaf_core::error::CoreError::Unauthorized(
            "Registro público indisponível: a empresa já possui um administrador".into(),
        )));
    }

    let user = state
        .auth_service
        .create(tenant.company_id, body.email, body.password, body.name, UserRole::Admin)
        .await?;

    let perms = resolve_perms(&state, &user).await;
    // Usuário recém-criado: token_version inicial é 0.
    let token = create_token(
        user.base.id,
        tenant.company_id,
        user.role.as_db_str(),
        perms.clone(),
        0,
        &state.config.jwt_secret,
        24,
    )?;

    Ok((StatusCode::CREATED, Json(AuthResponse { token, user: AuthUserPayload::from(&user), perms })))
}

/// POST /auth/login-desktop — autentica desktop sem TenantContext.
///
/// Regras aplicadas (AI_RULES.md §4, §11):
/// - Desktop envia apenas email + password (sem subdomínio)
/// - Servidor busca usuário globalmente por email e valida senha
/// - Resolve empresa (subdomain) a partir do company_id do usuário
/// - Exceção documentada ao §11: busca global necessária para
///   identificar o tenant antes da autenticação
async fn login_desktop(
    State(state): State<AppState>,
    ip: ClientIp,
    Json(body): Json<LoginRequest>,
) -> Result<Json<DesktopAuthResponse>, ServerError> {
    if !state.login_rate_limiter.check(ip.0) {
        return Err(ServerError::TooManyRequests(RATE_LIMIT_MSG));
    }
    let user = state
        .auth_service
        .authenticate_global(&body.email, &body.password)
        .await?;

    let company_id = user.base.company_id;

    let company = state
        .company_service
        .find_by_id(company_id)
        .await?
        .ok_or(ServerError::TenantNotFound)?;

    let perms = resolve_perms(&state, &user).await;
    let tv = state
        .auth_service
        .find_token_version(company_id, user.base.id)
        .await?
        .unwrap_or(0);
    let token = create_token(
        user.base.id,
        company_id,
        user.role.as_db_str(),
        perms.clone(),
        tv,
        &state.config.jwt_secret,
        24,
    )?;

    Ok(Json(DesktopAuthResponse {
        token,
        user: AuthUserPayload::from(&user),
        subdomain: company.subdomain,
        company_name: company.name,
        perms,
    }))
}

/// POST /auth/login — autentica e retorna token JWT.
async fn login(
    State(state): State<AppState>,
    tenant: TenantContext,
    ip: ClientIp,
    Json(body): Json<LoginRequest>,
) -> Result<Json<AuthResponse>, ServerError> {
    if !state.login_rate_limiter.check(ip.0) {
        return Err(ServerError::TooManyRequests(RATE_LIMIT_MSG));
    }
    let user = state
        .auth_service
        .authenticate(tenant.company_id, &body.email, &body.password)
        .await?;

    let perms = resolve_perms(&state, &user).await;
    let tv = state
        .auth_service
        .find_token_version(tenant.company_id, user.base.id)
        .await?
        .unwrap_or(0);
    let token = create_token(
        user.base.id,
        tenant.company_id,
        user.role.as_db_str(),
        perms.clone(),
        tv,
        &state.config.jwt_secret,
        24,
    )?;

    Ok(Json(AuthResponse { token, user: AuthUserPayload::from(&user), perms }))
}
