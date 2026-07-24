//! Painel do super admin — leitura da trilha de auditoria.

use axum::extract::State;
use axum::Json;
use serde::Serialize;

use crate::context::AppState;
use crate::error::ServerError;
use crate::middleware::auth::AuthClaims;

use super::require_super_admin;
// ── Auditoria ────────────────────────────────────────────────────────────
#[derive(Serialize)]
pub(super) struct AuditRowOut {
    actor: String,
    action: String,
    target: String,
    details: String,
    /// "DD/MM/AAAA HH:MM".
    at: String,
}

/// Trilha das últimas ações do super admin (somente leitura — §11).
pub(super) async fn list_audit(
    State(state): State<AppState>,
    auth: AuthClaims,
) -> Result<Json<Vec<AuditRowOut>>, ServerError> {
    require_super_admin(&auth)?;
    let entries = state.audit_service.find_recent(200).await?;
    Ok(Json(
        entries
            .into_iter()
            .map(|e| AuditRowOut {
                actor: e.actor_name,
                action: e.action,
                target: e.target_label,
                details: e.details,
                at: e.created_at.format("%d/%m/%Y %H:%M").to_string(),
            })
            .collect(),
    ))
}

