use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use letaf_core::auth::model::{User, UserRole};

use crate::context::AppState;
use crate::error::ServerError;
use crate::middleware::auth::AuthClaims;
use crate::middleware::tenant::TenantContext;

const PERM_VIEW: &str = "collaborators.view";
const PERM_EDIT: &str = "collaborators.edit";

/// Rotas de Funcionário (colaborador) — cadastro/edição com Função.
/// Gateadas por `collaborators.*` (Admin tem bypass).
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/collaborators", get(list).post(create))
        .route("/collaborators/{id}", get(get_one).put(update).delete(delete))
}

/// Payload do colaborador (sem `password_hash` nem metadados internos).
#[derive(Serialize)]
struct CollaboratorPayload {
    id: Uuid,
    name: String,
    email: String,
    role: UserRole,
    job_role_id: Option<Uuid>,
}

impl From<&User> for CollaboratorPayload {
    fn from(u: &User) -> Self {
        Self {
            id: u.base.id,
            name: u.name.clone(),
            email: u.email.clone(),
            role: u.role,
            job_role_id: u.job_role_id,
        }
    }
}

#[derive(Deserialize)]
struct CreateCollaboratorRequest {
    name: String,
    email: String,
    password: String,
    #[serde(default)]
    job_role_id: Option<Uuid>,
}

#[derive(Deserialize)]
struct UpdateCollaboratorRequest {
    name: String,
    #[serde(default)]
    job_role_id: Option<Uuid>,
    /// Quando presente e não-vazia, troca a senha. `None` mantém a atual.
    #[serde(default)]
    password: Option<String>,
}

async fn list(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
) -> Result<Json<Vec<CollaboratorPayload>>, ServerError> {
    auth.verify_company(tenant.company_id)?;
    auth.require_permission(PERM_VIEW)?;
    let users = state.auth_service.find_all(tenant.company_id).await?;
    Ok(Json(users.iter().map(CollaboratorPayload::from).collect()))
}

async fn get_one(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
) -> Result<Json<CollaboratorPayload>, ServerError> {
    auth.verify_company(tenant.company_id)?;
    auth.require_permission(PERM_VIEW)?;
    let user = state
        .auth_service
        .find_by_id(tenant.company_id, id)
        .await?
        .ok_or_else(|| ServerError::Core(letaf_core::error::CoreError::NotFound("Funcionário não encontrado".into())))?;
    Ok(Json(CollaboratorPayload::from(&user)))
}

async fn create(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Json(body): Json<CreateCollaboratorRequest>,
) -> Result<(StatusCode, Json<CollaboratorPayload>), ServerError> {
    auth.verify_company(tenant.company_id)?;
    auth.require_permission(PERM_EDIT)?;
    let user = state
        .auth_service
        .create_employee(tenant.company_id, body.email, body.password, body.name, body.job_role_id)
        .await?;
    Ok((StatusCode::CREATED, Json(CollaboratorPayload::from(&user))))
}

async fn update(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateCollaboratorRequest>,
) -> Result<Json<CollaboratorPayload>, ServerError> {
    auth.verify_company(tenant.company_id)?;
    auth.require_permission(PERM_EDIT)?;
    let user = state
        .auth_service
        .update_employee(tenant.company_id, id, body.name, body.job_role_id, body.password)
        .await?;
    Ok(Json(CollaboratorPayload::from(&user)))
}

async fn delete(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ServerError> {
    auth.verify_company(tenant.company_id)?;
    auth.require_permission(PERM_EDIT)?;
    // Não permite excluir a si mesmo (evita o admin se trancar fora).
    if auth.0.sub == id {
        return Err(ServerError::Core(letaf_core::error::CoreError::Validation(
            "Você não pode excluir o próprio usuário".into(),
        )));
    }
    // `delete_employee` recusa alvos Admin/SuperAdmin (§11 — impede um gerente
    // com `collaborators.edit` de excluir o Admin do tenant).
    state.auth_service.delete_employee(tenant.company_id, id).await?;
    Ok(Json(json!({ "deleted": true })))
}
