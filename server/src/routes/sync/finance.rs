use axum::extract::{Query, State};
use axum::Json;
use serde_json::{json, Value};

use letaf_core::cash::model::{CashMovement, CashSession};
use letaf_core::finance::model::FinanceEntry;
use letaf_core::finance_category::model::FinanceCategory;
use letaf_core::wallet::model::{WalletAccount, WalletMovement};

use crate::context::AppState;
use crate::error::ServerError;
use crate::jwt::ROLES_OPERATORS;
use crate::middleware::auth::AuthClaims;

use super::PullQuery;

/// POST /sync/cash-sessions — upsert de sessão de caixa (desktop → servidor).
pub(crate) async fn sync_cash_session(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(session): Json<CashSession>,
) -> Result<Json<Value>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    auth.require_permission("cash.view")?;
    state.cash_service
        .sync_upsert_session(auth.0.company_id, session)
        .await?;
    Ok(Json(json!({ "synced": true })))
}

/// GET /sync/pull/cash-sessions?since=<timestamp>
pub(crate) async fn pull_cash_sessions(
    State(state): State<AppState>,
    auth: AuthClaims,
    Query(params): Query<PullQuery>,
) -> Result<Json<Vec<CashSession>>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    // Dado sensível (caixa): pull e push exigem cash.view (§11) — operadores
    // do PDV têm a permissão, então não trava o fluxo do caixa.
    auth.require_permission("cash.view")?;
    let items = state.cash_service
        .find_sessions_updated_since_paged(
            auth.0.company_id,
            params.since,
            params.after_id(),
            params.page_limit(),
        )
        .await?;
    Ok(Json(items))
}

/// POST /sync/cash-movements — upsert de movimento (depois de cash-sessions
/// pela FK lógica `session_id`).
pub(crate) async fn sync_cash_movement(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(movement): Json<CashMovement>,
) -> Result<Json<Value>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    auth.require_permission("cash.view")?;
    state.cash_service
        .sync_upsert_movement(auth.0.company_id, movement)
        .await?;
    Ok(Json(json!({ "synced": true })))
}

/// GET /sync/pull/cash-movements?since=<timestamp>
pub(crate) async fn pull_cash_movements(
    State(state): State<AppState>,
    auth: AuthClaims,
    Query(params): Query<PullQuery>,
) -> Result<Json<Vec<CashMovement>>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    auth.require_permission("cash.view")?;
    let items = state.cash_service
        .find_movements_updated_since_paged(
            auth.0.company_id,
            params.since,
            params.after_id(),
            params.page_limit(),
        )
        .await?;
    Ok(Json(items))
}

/// POST /sync/finance-categories — upsert de categoria financeira.
pub(crate) async fn sync_finance_category(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(category): Json<FinanceCategory>,
) -> Result<Json<Value>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    auth.require_permission("finance.edit")?;
    state
        .finance_category_service
        .sync_upsert(auth.0.company_id, category)
        .await?;
    Ok(Json(json!({ "synced": true })))
}

/// GET /sync/pull/finance-categories?since=<timestamp>
pub(crate) async fn pull_finance_categories(
    State(state): State<AppState>,
    auth: AuthClaims,
    Query(params): Query<PullQuery>,
) -> Result<Json<Vec<FinanceCategory>>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    auth.require_permission("finance.view")?;
    let items = state
        .finance_category_service
        .find_updated_since(auth.0.company_id, params.since)
        .await?;
    Ok(Json(items))
}

/// POST /sync/finance-entries — upsert de lançamento financeiro.
/// Lançamentos parcelados/recorrentes são enviados um a um pelo
/// sync worker; o servidor não precisa tratá-los em batch porque já
/// vieram com seu `parent_id` definido pelo desktop.
pub(crate) async fn sync_finance_entry(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(entry): Json<FinanceEntry>,
) -> Result<Json<Value>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    auth.require_permission("finance.edit")?;
    state
        .finance_service
        .sync_upsert(auth.0.company_id, entry)
        .await?;
    Ok(Json(json!({ "synced": true })))
}

/// GET /sync/pull/finance-entries?since=<timestamp>
pub(crate) async fn pull_finance_entries(
    State(state): State<AppState>,
    auth: AuthClaims,
    Query(params): Query<PullQuery>,
) -> Result<Json<Vec<FinanceEntry>>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    auth.require_permission("finance.view")?;
    let items = state
        .finance_service
        .find_updated_since_paged(
            auth.0.company_id,
            params.since,
            params.after_id(),
            params.page_limit(),
        )
        .await?;
    Ok(Json(items))
}

/// POST /sync/wallet-accounts — upsert de carteira do cliente.
pub(crate) async fn sync_wallet_account(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(account): Json<WalletAccount>,
) -> Result<Json<Value>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    auth.require_permission("customers.edit")?;
    state
        .wallet_service
        .sync_upsert_account(auth.0.company_id, account)
        .await?;
    Ok(Json(json!({ "synced": true })))
}

/// GET /sync/pull/wallet-accounts?since=<timestamp>
pub(crate) async fn pull_wallet_accounts(
    State(state): State<AppState>,
    auth: AuthClaims,
    Query(params): Query<PullQuery>,
) -> Result<Json<Vec<WalletAccount>>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    // Carteira = crédito (fiado) por cliente: gateia pela permissão de clientes.
    auth.require_permission("customers.view")?;
    let items = state
        .wallet_service
        .find_accounts_updated_since_paged(
            auth.0.company_id,
            params.since,
            params.after_id(),
            params.page_limit(),
        )
        .await?;
    Ok(Json(items))
}

/// POST /sync/wallet-movements — upsert de movimento (depois de
/// wallet-accounts, pela FK lógica `account_id`).
pub(crate) async fn sync_wallet_movement(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(movement): Json<WalletMovement>,
) -> Result<Json<Value>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    auth.require_permission("customers.edit")?;
    state
        .wallet_service
        .sync_upsert_movement(auth.0.company_id, movement)
        .await?;
    Ok(Json(json!({ "synced": true })))
}

/// GET /sync/pull/wallet-movements?since=<timestamp>
pub(crate) async fn pull_wallet_movements(
    State(state): State<AppState>,
    auth: AuthClaims,
    Query(params): Query<PullQuery>,
) -> Result<Json<Vec<WalletMovement>>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    auth.require_permission("customers.view")?;
    let items = state
        .wallet_service
        .find_movements_updated_since_paged(
            auth.0.company_id,
            params.since,
            params.after_id(),
            params.page_limit(),
        )
        .await?;
    Ok(Json(items))
}
