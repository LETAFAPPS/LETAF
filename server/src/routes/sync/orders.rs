use axum::extract::{Query, State};
use axum::Json;
use serde_json::{json, Value};

use letaf_core::order::model::Order;

use crate::context::AppState;
use crate::error::ServerError;
use crate::jwt::ROLES_OPERATORS;
use crate::middleware::auth::AuthClaims;

use super::PullQuery;

/// POST /sync/orders — upsert de pedido sincronizado (desktop → servidor).
///
/// Regras aplicadas (AI_RULES.md §7, §11):
/// - Apenas operador (`ROLE_USER`) pode fazer push de pedidos
/// - Isolamento por `company_id` via `sync_upsert` do service
pub(crate) async fn sync_order(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(order): Json<Order>,
) -> Result<Json<Value>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    state
        .order_service
        .sync_upsert(auth.0.company_id, order)
        .await?;

    Ok(Json(json!({ "synced": true })))
}

/// GET /sync/pull/orders?since=<timestamp> — pull de pedidos para o desktop.
pub(crate) async fn pull_orders(
    State(state): State<AppState>,
    auth: AuthClaims,
    Query(params): Query<PullQuery>,
) -> Result<Json<Vec<Order>>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    let items = state
        .order_service
        .find_updated_since_paged(
            auth.0.company_id,
            params.since,
            params.after_id(),
            params.page_limit(),
        )
        .await?;

    Ok(Json(items))
}
