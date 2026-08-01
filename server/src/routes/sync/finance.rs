use axum::extract::{Query, State};
use axum::Json;
use serde_json::{json, Value};

use letaf_core::cash::model::{CashMovement, CashSession};
use letaf_core::finance::model::FinanceEntry;
use letaf_core::finance_category::model::FinanceCategory;
use letaf_core::treasury::model::{Treasury, TreasuryMovement};
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
    // `finance.edit` também: dar baixa num recebimento no Financeiro
    // lança o movimento na sessão de caixa. Sem isso, quem opera o
    // Financeiro sem acesso ao Caixa levava 403 eterno e o fechamento no
    // servidor nunca batia com o do desktop.
    auth.require_any_permission(&["cash.view", "finance.edit"])?;
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
    // Idem à sessão: a baixa de um recebimento no Financeiro cria o
    // movimento de caixa. Sem `finance.edit` aqui, a sessão subia e os
    // movimentos dela levavam 403 eterno — o fechamento no servidor nunca
    // batia com o do desktop de origem.
    auth.require_any_permission(&["cash.view", "finance.edit"])?;
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
    // `pdv.view` entra na lista porque a venda FIADO gera o espelho
    // automático da dívida em `finance_entries`; sem isso o caixa levava
    // 403 a cada ciclo e a conta a receber nunca chegava ao servidor.
    auth.require_any_permission(&["finance.edit", "pdv.view"])?;
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

/// POST /sync/treasury-accounts — upsert da carteira do estabelecimento
/// (tesouraria, singleton por empresa). O `company_id` vem SEMPRE do JWT
/// (§11) — o service rejeita payload de outro tenant.
pub(crate) async fn sync_treasury_account(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(treasury): Json<Treasury>,
) -> Result<Json<Value>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    auth.require_permission("finance.edit")?;
    state
        .treasury_service
        .sync_upsert(auth.0.company_id, treasury)
        .await?;
    Ok(Json(json!({ "synced": true })))
}

/// GET /sync/pull/treasury-accounts?since=<timestamp> — singleton por
/// empresa: pull simples (sem paginação keyset).
pub(crate) async fn pull_treasury_accounts(
    State(state): State<AppState>,
    auth: AuthClaims,
    Query(params): Query<PullQuery>,
) -> Result<Json<Vec<Treasury>>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    auth.require_permission("finance.view")?;
    let items = state
        .treasury_service
        .find_updated_since(auth.0.company_id, params.since)
        .await?;
    Ok(Json(items))
}

/// POST /sync/treasury-movements — upsert de movimento manual da
/// carteira do estabelecimento. `company_id` sempre do JWT (§11).
pub(crate) async fn sync_treasury_movement(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(movement): Json<TreasuryMovement>,
) -> Result<Json<Value>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    auth.require_permission("finance.edit")?;
    state
        .treasury_service
        .sync_upsert_movement(auth.0.company_id, movement)
        .await?;
    Ok(Json(json!({ "synced": true })))
}

/// GET /sync/pull/treasury-movements?since=<timestamp> — movimentos
/// manuais do caixa da empresa (volume baixo: pull simples).
pub(crate) async fn pull_treasury_movements(
    State(state): State<AppState>,
    auth: AuthClaims,
    Query(params): Query<PullQuery>,
) -> Result<Json<Vec<TreasuryMovement>>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    auth.require_permission("finance.view")?;
    let items = state
        .treasury_service
        .find_movements_updated_since_paged(
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
    // Idem: a cobrança fiada do PDV move a carteira do cliente.
    auth.require_any_permission(&["customers.edit", "pdv.view"])?;
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
    // Idem: a cobrança fiada do PDV move a carteira do cliente.
    auth.require_any_permission(&["customers.edit", "pdv.view"])?;
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
