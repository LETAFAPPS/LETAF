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

use super::{email_available, EMAIL_TAKEN};
// ── Administradores (gestão dos super admins) ────────────────────────────
#[derive(Serialize)]
pub(super) struct AdminRow {
    id: Uuid,
    name: String,
    email: String,
    /// Função de administrador (id + nome). Vazio = master (acesso total).
    role_id: String,
    role_name: String,
}

pub(super) async fn list_admins(
    State(state): State<AppState>,
    auth: AuthClaims,
) -> Result<Json<Vec<AdminRow>>, ServerError> {
    auth.require_screen("admins")?;
    let users: Vec<_> = state
        .auth_service
        .find_all(auth.0.company_id)
        .await?
        .into_iter()
        .filter(|u| u.role.is_super_admin())
        .collect();
    // Atribuições usuário→função e nomes das funções (sem N+1).
    let ids: Vec<Uuid> = users.iter().map(|u| u.base.id).collect();
    let assignments = state.admin_role_service.roles_of_users(&ids).await.unwrap_or_default();
    let user_role: std::collections::HashMap<Uuid, Uuid> = assignments.into_iter().collect();
    let role_name: std::collections::HashMap<Uuid, String> = state
        .admin_role_service
        .find_all()
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|r| (r.id, r.name))
        .collect();
    let rows = users
        .into_iter()
        .map(|u| {
            let rid = user_role.get(&u.base.id);
            AdminRow {
                id: u.base.id,
                name: u.name,
                email: u.email,
                role_id: rid.map(|r| r.to_string()).unwrap_or_default(),
                role_name: rid.and_then(|r| role_name.get(r)).cloned().unwrap_or_default(),
            }
        })
        .collect();
    Ok(Json(rows))
}

#[derive(Deserialize)]
pub(super) struct CreateAdminRequest {
    name: String,
    email: String,
    password: String,
    /// Função de administrador (id do catálogo). Vazio = master.
    #[serde(default)]
    admin_role_id: Option<String>,
}

pub(super) async fn create_admin(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(body): Json<CreateAdminRequest>,
) -> Result<(StatusCode, Json<Value>), ServerError> {
    auth.require_screen("admins")?;
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
    // Atribui a função escolhida (best-effort — o admin já foi criado).
    let role_id = body.admin_role_id.as_deref().and_then(|s| Uuid::parse_str(s).ok());
    let _ = state.admin_role_service.set_user_role(user.base.id, role_id).await;
    Ok((StatusCode::CREATED, Json(json!({ "id": user.base.id }))))
}

#[derive(Deserialize)]
pub(super) struct UpdateAdminRequest {
    name: String,
    email: String,
    /// Nova senha; vazio/ausente mantém a atual.
    #[serde(default)]
    password: Option<String>,
    /// Função de administrador (id). Vazio/ausente = master (sem restrição).
    #[serde(default)]
    admin_role_id: Option<String>,
}

pub(super) async fn update_admin(
    State(state): State<AppState>,
    auth: AuthClaims,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateAdminRequest>,
) -> Result<Json<Value>, ServerError> {
    auth.require_screen("admins")?;
    if !email_available(&state, &body.email, Some(id)).await {
        return Err(ServerError::Core(CoreError::Validation(EMAIL_TAKEN.into())));
    }
    let role_id = body.admin_role_id.as_deref().and_then(|s| Uuid::parse_str(s).ok());
    state
        .auth_service
        // Painel do super admin não mexe na foto do operador → None.
        .update_credentials(auth.0.company_id, id, body.email, body.name, body.password, None)
        .await?;
    // `None` remove a função (vira master). Best-effort.
    let _ = state.admin_role_service.set_user_role(id, role_id).await;
    Ok(Json(json!({ "ok": true })))
}

pub(super) async fn delete_admin(
    State(state): State<AppState>,
    auth: AuthClaims,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ServerError> {
    auth.require_screen("admins")?;
    // Não pode remover a si mesmo.
    if id == auth.0.sub {
        return Err(ServerError::Core(CoreError::Validation(
            "Você não pode remover o próprio usuário.".into(),
        )));
    }
    // O super admin MASTER (sem função = acesso total) não pode ser excluído.
    if state.admin_role_service.role_for_user(id).await?.is_none() {
        return Err(ServerError::Core(CoreError::Validation(
            "O super admin master não pode ser excluído.".into(),
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
    // Remove a atribuição de função e o usuário.
    let _ = state.admin_role_service.set_user_role(id, None).await;
    state.auth_service.soft_delete(auth.0.company_id, id).await?;
    Ok(Json(json!({ "ok": true })))
}

