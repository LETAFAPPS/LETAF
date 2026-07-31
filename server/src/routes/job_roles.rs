use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use letaf_core::job_role::model::JobRole;

use crate::context::AppState;
use crate::error::ServerError;
use crate::middleware::auth::AuthClaims;
use crate::middleware::tenant::TenantContext;

/// Permissões de gestão de colaboradores (RBAC). Admin tem bypass.
const PERM_VIEW: &str = "collaborators.view";
const PERM_EDIT: &str = "collaborators.edit";

/// Rotas de Função (cargo) + catálogo de permissões. Gateadas por
/// `collaborators.*` (§11 — o servidor é a autoridade).
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/permissions", get(catalog))
        .route("/job-roles", get(list).post(create))
        .route("/job-roles/{id}", get(get_one).put(update).delete(delete))
}

#[derive(Deserialize)]
struct JobRoleRequest {
    name: String,
    #[serde(default)]
    permissions: Vec<String>,
}

/// GET /permissions — catálogo (feature, rótulo, ações) para a UI montar
/// os checkboxes. Não expõe dados de empresa; só requer ser operador
/// com acesso a colaboradores.
async fn catalog(
    auth: AuthClaims,
    tenant: TenantContext,
) -> Result<Json<Value>, ServerError> {
    auth.verify_company(tenant.company_id)?;
    auth.require_permission(PERM_VIEW)?;
    let features: Vec<Value> = letaf_core::permission::FEATURES
        .iter()
        .map(|(key, label, has_edit)| json!({ "key": key, "label": label, "has_edit": has_edit }))
        .collect();
    Ok(Json(json!({ "features": features })))
}

async fn list(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
) -> Result<Json<Vec<JobRole>>, ServerError> {
    auth.verify_company(tenant.company_id)?;
    auth.require_permission(PERM_VIEW)?;
    Ok(Json(state.job_role_service.find_all(tenant.company_id).await?))
}

async fn get_one(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
) -> Result<Json<JobRole>, ServerError> {
    auth.verify_company(tenant.company_id)?;
    auth.require_permission(PERM_VIEW)?;
    let item = state
        .job_role_service
        .find_by_id(tenant.company_id, id)
        .await?
        .ok_or_else(|| ServerError::Core(letaf_core::error::CoreError::NotFound("Função não encontrada".into())))?;
    Ok(Json(item))
}

/// O chamador só mexe numa Função cujas permissões ATUAIS ele possua.
async fn require_can_touch_role(
    state: &AppState,
    auth: &AuthClaims,
    company_id: Uuid,
    id: Uuid,
) -> Result<(), ServerError> {
    let Some(role) = state.job_role_service.find_by_id(company_id, id).await? else {
        return Ok(());
    };
    auth.require_can_grant(&role.permissions)
}

async fn create(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Json(body): Json<JobRoleRequest>,
) -> Result<(StatusCode, Json<JobRole>), ServerError> {
    auth.verify_company(tenant.company_id)?;
    auth.require_permission(PERM_EDIT)?;
    auth.require_can_grant(&body.permissions)?;
    let item = state
        .job_role_service
        .create(tenant.company_id, body.name, body.permissions)
        .await?;
    Ok((StatusCode::CREATED, Json(item)))
}

async fn update(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
    Json(body): Json<JobRoleRequest>,
) -> Result<Json<JobRole>, ServerError> {
    auth.verify_company(tenant.company_id)?;
    auth.require_permission(PERM_EDIT)?;
    auth.require_can_grant(&body.permissions)?;
    // Também sobre as permissões ATUAIS: sem isso um funcionário
    // reescrevia a Função "Gerente" reduzindo-a ao próprio conjunto —
    // não subia o dele, mas derrubava o do gerente na hora
    // (`bump_token_version_by_job_role` logo abaixo).
    require_can_touch_role(&state, &auth, tenant.company_id, id).await?;
    let item = state
        .job_role_service
        .update(tenant.company_id, id, body.name, body.permissions)
        .await?;
    // As permissões da função podem ter mudado → revoga os tokens de quem a
    // possui, para os novos limites valerem já no próximo request (§11).
    state
        .auth_service
        .bump_token_version_by_job_role(tenant.company_id, id)
        .await?;
    Ok(Json(item))
}

async fn delete(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ServerError> {
    auth.verify_company(tenant.company_id)?;
    auth.require_permission(PERM_EDIT)?;
    // Apagar a Função deixa quem a tinha SEM permissão nenhuma no
    // próximo login — negação de serviço por delegação se o alvo for
    // mais poderoso que o chamador.
    require_can_touch_role(&state, &auth, tenant.company_id, id).await?;
    state.job_role_service.soft_delete(tenant.company_id, id).await?;
    Ok(Json(json!({ "deleted": true })))
}
