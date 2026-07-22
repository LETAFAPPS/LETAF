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
    // Criar/sincronizar pedido é capacidade tanto do gestor (`orders.view`)
    // quanto do CAIXA que operou o PDV (`pdv.view`). O desktop libera o PDV por
    // `pdv.view`; sem aceitá-lo aqui, um caixa com `pdv.view` sem `orders.view`
    // criava a venda offline mas o push voltava 403 para sempre — dado preso.
    // Não concede leitura (o pull segue gateado); só permite subir o que ele já
    // podia criar no PDV. §11.
    auth.require_any_permission(&["orders.view", "pdv.view"])?;
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
