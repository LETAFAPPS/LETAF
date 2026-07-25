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
    /// Foto de perfil (base64) ou "" — thumbnail do card.
    avatar: String,
    /// Acesso ativo (`false` = desativado, não loga). Master é sempre ativo.
    active: bool,
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
    // Atribuições usuário→função, nomes e status ativo (sem N+1).
    let ids: Vec<Uuid> = users.iter().map(|u| u.base.id).collect();
    let assignments = state.admin_role_service.roles_of_users(&ids).await.unwrap_or_default();
    let user_role: std::collections::HashMap<Uuid, Uuid> = assignments.into_iter().collect();
    let active_map: std::collections::HashMap<Uuid, bool> = state
        .admin_role_service
        .active_of_users(&ids)
        .await
        .unwrap_or_default()
        .into_iter()
        .collect();
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
                active: active_map.get(&u.base.id).copied().unwrap_or(true),
                avatar: u.avatar.clone().unwrap_or_default(),
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

/// Ativa/desativa o acesso de um admin restrito. O master (sem função) e o
/// próprio usuário logado não podem ser desativados (§11).
#[derive(Deserialize)]
pub(super) struct SetActiveRequest {
    active: bool,
}

pub(super) async fn set_admin_active(
    State(state): State<AppState>,
    auth: AuthClaims,
    Path(id): Path<Uuid>,
    Json(body): Json<SetActiveRequest>,
) -> Result<Json<Value>, ServerError> {
    auth.require_screen("admins")?;
    if !body.active && id == auth.0.sub {
        return Err(ServerError::Core(CoreError::Validation(
            "Você não pode desativar o próprio usuário.".into(),
        )));
    }
    // Master (sem função) é sempre ativo — não pode ser desativado.
    if !body.active && state.admin_role_service.role_for_user(id).await?.is_none() {
        return Err(ServerError::Core(CoreError::Validation(
            "O super admin master não pode ser desativado.".into(),
        )));
    }
    state.admin_role_service.set_user_active(id, body.active).await?;
    Ok(Json(json!({ "ok": true })))
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
    // §11: novos usuários SEMPRE têm uma função cadastrada. O master (acesso
    // total, sem função) é único e só existe no banco — não se cria pela API.
    let role_id = require_valid_role(&state, body.admin_role_id.as_deref()).await?;
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
    let _ = state.admin_role_service.set_user_role(user.base.id, Some(role_id)).await;
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
    // O master (sem função) é sempre Super Admin — a função é IMUTÁVEL (§11):
    // valida a nova função só para os demais; ao master ninguém pode ser
    // rebaixado nem promovido.
    let is_master = state.admin_role_service.role_for_user(id).await?.is_none();
    let new_role = if is_master {
        None
    } else {
        Some(require_valid_role(&state, body.admin_role_id.as_deref()).await?)
    };
    state
        .auth_service
        // Painel do super admin não mexe na foto do operador → None.
        .update_credentials(auth.0.company_id, id, body.email, body.name, body.password, None)
        .await?;
    // Só reatribui função para não-master; o master permanece intacto.
    if let Some(role_id) = new_role {
        let _ = state.admin_role_service.set_user_role(id, Some(role_id)).await;
    }
    Ok(Json(json!({ "ok": true })))
}

/// Valida a função escolhida para um usuário restrito: precisa ser um id
/// existente no catálogo. Vazio/ausente/inexistente → erro (nunca vira
/// master — §11: só há um super admin master, criado direto no banco).
async fn require_valid_role(
    state: &AppState,
    admin_role_id: Option<&str>,
) -> Result<Uuid, ServerError> {
    let role_id = admin_role_id
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| CoreError::Validation("Selecione uma função para o usuário.".into()))?;
    if state.admin_role_service.find_by_id(role_id).await?.is_none() {
        return Err(ServerError::Core(CoreError::Validation("Função inválida.".into())));
    }
    Ok(role_id)
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

