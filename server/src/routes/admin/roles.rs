//! Painel do super admin — Funções de administrador (RBAC do painel).
//! Cada função define quais TELAS do painel um usuário acessa. Gate por
//! `require_screen("roles")` (§11).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use letaf_core::error::CoreError;

use crate::context::AppState;
use crate::error::ServerError;
use crate::middleware::auth::AuthClaims;

use super::audit;

#[derive(Serialize)]
pub(super) struct RolePayload {
    id: Uuid,
    name: String,
    screens: Vec<String>,
    /// Quantos usuários usam esta função.
    users: i64,
}

pub(super) async fn list_roles(
    State(state): State<AppState>,
    auth: AuthClaims,
) -> Result<Json<Vec<RolePayload>>, ServerError> {
    auth.require_screen("roles")?;
    let roles = state.admin_role_service.find_all().await?;
    let mut out = Vec::with_capacity(roles.len());
    for r in roles {
        let users = state.admin_role_service.count_users(r.id).await.unwrap_or(0);
        out.push(RolePayload { id: r.id, name: r.name, screens: r.screens, users });
    }
    Ok(Json(out))
}

#[derive(Deserialize)]
pub(super) struct RoleBody {
    name: String,
    #[serde(default)]
    screens: Vec<String>,
}

pub(super) async fn create_role(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(body): Json<RoleBody>,
) -> Result<(StatusCode, Json<Value>), ServerError> {
    auth.require_screen("roles")?;
    auth.require_can_grant_screens(&body.screens)?;
    let role = state.admin_role_service.create(body.name, body.screens).await?;
    audit(&state, &auth, "role.create", "admin_role", Some(role.id), role.name.clone(), String::new()).await;
    Ok((StatusCode::CREATED, Json(json!({ "id": role.id }))))
}

pub(super) async fn update_role(
    State(state): State<AppState>,
    auth: AuthClaims,
    Path(id): Path<Uuid>,
    Json(body): Json<RoleBody>,
) -> Result<Json<Value>, ServerError> {
    auth.require_screen("roles")?;
    auth.require_can_grant_screens(&body.screens)?;
    // E sobre as telas ATUAIS da função — senão dava para reescrever uma
    // função mais poderosa (inclusive a própria) sem tê-las.
    if let Some(current) = state.admin_role_service.find_by_id(id).await? {
        auth.require_can_grant_screens(&current.screens)?;
    }
    let role = state.admin_role_service.update(id, body.name, body.screens).await?;
    audit(&state, &auth, "role.update", "admin_role", Some(id), role.name, String::new()).await;
    Ok(Json(json!({ "ok": true })))
}

pub(super) async fn delete_role(
    State(state): State<AppState>,
    auth: AuthClaims,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ServerError> {
    auth.require_screen("roles")?;
    if let Some(current) = state.admin_role_service.find_by_id(id).await? {
        auth.require_can_grant_screens(&current.screens)?;
    }
    let in_use = state.admin_role_service.count_users(id).await?;
    if in_use > 0 {
        return Err(ServerError::Core(CoreError::Validation(format!(
            "Função em uso por {in_use} usuário(s). Reatribua-os antes de excluir."
        ))));
    }
    state.admin_role_service.soft_delete(id).await?;
    audit(&state, &auth, "role.delete", "admin_role", Some(id), String::new(), String::new()).await;
    Ok(Json(json!({ "ok": true })))
}
