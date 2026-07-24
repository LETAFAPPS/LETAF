//! Painel do super admin — gestão dos próprios super admins.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use letaf_core::auth::model::UserRole;
use letaf_core::error::CoreError;

use crate::context::AppState;
use crate::error::ServerError;
use crate::middleware::auth::AuthClaims;

use super::{email_available, require_super_admin, EMAIL_TAKEN};
// ── Administradores (gestão dos super admins) ────────────────────────────
#[derive(Serialize)]
pub(super) struct AdminRow {
    id: Uuid,
    name: String,
    email: String,
}

pub(super) async fn list_admins(
    State(state): State<AppState>,
    auth: AuthClaims,
) -> Result<Json<Vec<AdminRow>>, ServerError> {
    require_super_admin(&auth)?;
    let users = state.auth_service.find_all(auth.0.company_id).await?;
    let rows = users
        .into_iter()
        .filter(|u| u.role.is_super_admin())
        .map(|u| AdminRow {
            id: u.base.id,
            name: u.name,
            email: u.email,
        })
        .collect();
    Ok(Json(rows))
}



#[derive(Deserialize)]
pub(super) struct CreateAdminRequest {
    name: String,
    email: String,
    password: String,
}

pub(super) async fn create_admin(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(body): Json<CreateAdminRequest>,
) -> Result<(StatusCode, Json<Value>), ServerError> {
    require_super_admin(&auth)?;
    if !email_available(&state, &body.email, None).await {
        return Err(ServerError::Core(CoreError::Validation(EMAIL_TAKEN.into())));
    }
    let user = state
        .auth_service
        .create(
            auth.0.company_id,
            body.email,
            body.password,
            body.name,
            UserRole::SuperAdmin,
        )
        .await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": user.base.id }))))
}

#[derive(Deserialize)]
pub(super) struct UpdateAdminRequest {
    name: String,
    email: String,
    /// Nova senha; vazio/ausente mantém a atual.
    #[serde(default)]
    password: Option<String>,
}

pub(super) async fn update_admin(
    State(state): State<AppState>,
    auth: AuthClaims,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateAdminRequest>,
) -> Result<Json<Value>, ServerError> {
    require_super_admin(&auth)?;
    if !email_available(&state, &body.email, Some(id)).await {
        return Err(ServerError::Core(CoreError::Validation(EMAIL_TAKEN.into())));
    }
    state
        .auth_service
        // Painel do super admin não mexe na foto do operador → None.
        .update_credentials(auth.0.company_id, id, body.email, body.name, body.password, None)
        .await?;
    Ok(Json(json!({ "ok": true })))
}

pub(super) async fn delete_admin(
    State(state): State<AppState>,
    auth: AuthClaims,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ServerError> {
    require_super_admin(&auth)?;
    // Não pode remover a si mesmo.
    if id == auth.0.sub {
        return Err(ServerError::Core(CoreError::Validation(
            "Você não pode remover o próprio usuário.".into(),
        )));
    }
    // Não pode remover o último super admin (não deixar a plataforma sem acesso).
    let admins = state.auth_service.find_all(auth.0.company_id).await?;
    let count = admins.iter().filter(|u| u.role.is_super_admin()).count();
    if count <= 1 {
        return Err(ServerError::Core(CoreError::Validation(
            "Deve existir ao menos um administrador.".into(),
        )));
    }
    state.auth_service.soft_delete(auth.0.company_id, id).await?;
    Ok(Json(json!({ "ok": true })))
}

