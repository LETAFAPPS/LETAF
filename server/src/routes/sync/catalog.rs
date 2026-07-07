use axum::extract::{Query, State};
use axum::Json;
use serde_json::{json, Value};

use letaf_core::addon::model::Addon;
use letaf_core::addon_group::model::AddonGroup;
use letaf_core::banner::model::Banner;
use letaf_core::coupon::model::Coupon;
use letaf_core::category::model::Category;
use letaf_core::job_role::model::JobRole;
use letaf_core::subcategory::model::Subcategory;
use letaf_core::product::model::Product;
use letaf_core::product::stock_movement::StockMovement;

use letaf_core::reconcile::{ManifestEntry, ReconcileRepository};

use crate::context::AppState;
use crate::error::ServerError;
use crate::jwt::ROLES_OPERATORS;
use crate::middleware::auth::AuthClaims;
use crate::repository::reconcile::PgReconcileRepository;

use super::PullQuery;

/// Query da rota de manifesto: qual entidade reconciliar.
#[derive(serde::Deserialize)]
pub(crate) struct ManifestQuery {
    pub(crate) entity: String,
}

/// GET /sync/reconcile/manifest?entity=<tabela> — manifesto (id, updated_at,
/// deleted_at) de TODAS as linhas da entidade para o tenant. Base da
/// reconciliação anti-entropia (§7): o desktop compara com o manifesto local
/// e sincroniza divergências/faltas nos dois sentidos.
pub(crate) async fn reconcile_manifest(
    State(state): State<AppState>,
    auth: AuthClaims,
    Query(q): Query<ManifestQuery>,
) -> Result<Json<Vec<ManifestEntry>>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    let repo = PgReconcileRepository::new(state.pool.clone());
    let manifest = repo.manifest(auth.0.company_id, &q.entity).await?;
    Ok(Json(manifest))
}

/// POST /sync/products — upsert de produto sincronizado.
pub(crate) async fn sync_product(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(product): Json<Product>,
) -> Result<Json<Value>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    state
        .product_service
        .sync_upsert(auth.0.company_id, product)
        .await?;

    Ok(Json(json!({ "synced": true })))
}

/// GET /sync/pull/products?since=<timestamp> — pull de produtos.
pub(crate) async fn pull_products(
    State(state): State<AppState>,
    auth: AuthClaims,
    Query(params): Query<PullQuery>,
) -> Result<Json<Vec<Product>>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    let items = state
        .product_service
        .find_updated_since_paged(
            auth.0.company_id,
            params.since,
            params.after_id(),
            params.page_limit(),
        )
        .await?;

    Ok(Json(items))
}

/// POST /sync/stock-movements — ingest de movimento de estoque (idempotente).
/// Aplica `stock_quantity += delta` uma única vez por id, substituindo o LWW
/// sobre o valor absoluto (evita overselling em vendas offline concorrentes).
pub(crate) async fn sync_stock_movement(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(movement): Json<StockMovement>,
) -> Result<Json<Value>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    state
        .product_service
        .apply_stock_movement(auth.0.company_id, movement)
        .await?;
    Ok(Json(json!({ "synced": true })))
}

/// GET /sync/pull/stock-movements?since=<timestamp>
pub(crate) async fn pull_stock_movements(
    State(state): State<AppState>,
    auth: AuthClaims,
    Query(params): Query<PullQuery>,
) -> Result<Json<Vec<StockMovement>>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    let items = state
        .product_service
        .find_stock_movements_updated_since_paged(
            auth.0.company_id,
            params.since,
            params.after_id(),
            params.page_limit(),
        )
        .await?;
    Ok(Json(items))
}

/// POST /sync/categories — upsert de categoria sincronizada.
pub(crate) async fn sync_category(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(category): Json<Category>,
) -> Result<Json<Value>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    state
        .category_service
        .sync_upsert(auth.0.company_id, category)
        .await?;

    Ok(Json(json!({ "synced": true })))
}

/// POST /sync/subcategories — upsert de subcategoria sincronizada.
pub(crate) async fn sync_subcategory(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(subcategory): Json<Subcategory>,
) -> Result<Json<Value>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    state
        .subcategory_service
        .sync_upsert(auth.0.company_id, subcategory)
        .await?;
    Ok(Json(json!({ "synced": true })))
}

/// GET /sync/pull/subcategories?since=<timestamp> — pull de subcategorias.
pub(crate) async fn pull_subcategories(
    State(state): State<AppState>,
    auth: AuthClaims,
    Query(params): Query<PullQuery>,
) -> Result<Json<Vec<Subcategory>>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    let items = state
        .subcategory_service
        .find_updated_since(auth.0.company_id, params.since)
        .await?;
    Ok(Json(items))
}

/// POST /sync/addon-groups — upsert de grupo de adicional (desktop → servidor).
pub(crate) async fn sync_addon_group(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(group): Json<AddonGroup>,
) -> Result<Json<Value>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    state.addon_group_service
        .sync_upsert(auth.0.company_id, group)
        .await?;
    Ok(Json(json!({ "synced": true })))
}

/// GET /sync/pull/addon-groups?since=<timestamp>
pub(crate) async fn pull_addon_groups(
    State(state): State<AppState>,
    auth: AuthClaims,
    Query(params): Query<PullQuery>,
) -> Result<Json<Vec<AddonGroup>>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    let items = state.addon_group_service
        .find_updated_since(auth.0.company_id, params.since)
        .await?;
    Ok(Json(items))
}

/// POST /sync/addons — upsert de adicional (desktop → servidor).
pub(crate) async fn sync_addon(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(addon): Json<Addon>,
) -> Result<Json<Value>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    state.addon_service
        .sync_upsert(auth.0.company_id, addon)
        .await?;
    Ok(Json(json!({ "synced": true })))
}

/// GET /sync/pull/addons?since=<timestamp>
pub(crate) async fn pull_addons(
    State(state): State<AppState>,
    auth: AuthClaims,
    Query(params): Query<PullQuery>,
) -> Result<Json<Vec<Addon>>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    let items = state.addon_service
        .find_updated_since(auth.0.company_id, params.since)
        .await?;
    Ok(Json(items))
}

/// POST /sync/banners — upsert de banner (desktop → servidor).
pub(crate) async fn sync_banner(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(banner): Json<Banner>,
) -> Result<Json<Value>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    state.banner_service
        .sync_upsert(auth.0.company_id, banner)
        .await?;
    Ok(Json(json!({ "synced": true })))
}

/// GET /sync/pull/banners?since=<timestamp>
pub(crate) async fn pull_banners(
    State(state): State<AppState>,
    auth: AuthClaims,
    Query(params): Query<PullQuery>,
) -> Result<Json<Vec<Banner>>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    let items = state.banner_service
        .find_updated_since(auth.0.company_id, params.since)
        .await?;
    Ok(Json(items))
}

/// POST /sync/coupons — upsert de cupom (desktop → servidor).
pub(crate) async fn sync_coupon(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(coupon): Json<Coupon>,
) -> Result<Json<Value>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    state.coupon_service
        .sync_upsert(auth.0.company_id, coupon)
        .await?;
    Ok(Json(json!({ "synced": true })))
}

/// GET /sync/pull/coupons?since=<timestamp>
pub(crate) async fn pull_coupons(
    State(state): State<AppState>,
    auth: AuthClaims,
    Query(params): Query<PullQuery>,
) -> Result<Json<Vec<Coupon>>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    let items = state.coupon_service
        .find_updated_since(auth.0.company_id, params.since)
        .await?;
    Ok(Json(items))
}


/// GET /sync/pull/categories?since=<timestamp> — pull de categorias.
pub(crate) async fn pull_categories(
    State(state): State<AppState>,
    auth: AuthClaims,
    Query(params): Query<PullQuery>,
) -> Result<Json<Vec<Category>>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    let items = state
        .category_service
        .find_updated_since(auth.0.company_id, params.since)
        .await?;

    Ok(Json(items))
}

/// POST /sync/job-roles — upsert de Função sincronizada (RBAC).
pub(crate) async fn sync_job_role(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(role): Json<JobRole>,
) -> Result<Json<Value>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    state
        .job_role_service
        .sync_upsert(auth.0.company_id, role)
        .await?;

    Ok(Json(json!({ "synced": true })))
}

/// GET /sync/pull/job-roles — Funções atualizadas desde `?since=`.
pub(crate) async fn pull_job_roles(
    State(state): State<AppState>,
    auth: AuthClaims,
    Query(params): Query<PullQuery>,
) -> Result<Json<Vec<JobRole>>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    let items = state
        .job_role_service
        .find_updated_since(auth.0.company_id, params.since)
        .await?;

    Ok(Json(items))
}
