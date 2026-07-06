use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{routing::get, Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use letaf_core::error::CoreError;
use letaf_core::product::model::{BalanceMode, Product};

use crate::context::AppState;
use crate::error::ServerError;
use crate::jwt::ROLES_OPERATORS;
use crate::middleware::auth::AuthClaims;
use crate::middleware::tenant::TenantContext;

/// Rotas REST para Product (protegidas por JWT).
///
/// Regras aplicadas (AI_RULES.md §1, §11, §12):
/// - GET → leitura
/// - POST → criação (201 Created)
/// - PUT → atualização
/// - DELETE → remoção lógica
/// - Respostas sempre em JSON
/// - Handler apenas converte HTTP ↔ domínio, sem lógica de negócio
/// - Autenticação obrigatória via JWT
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/products", get(list).post(create))
        .route("/products/{id}", get(get_one).put(update).delete(delete))
}

#[derive(Deserialize)]
struct CreateProductRequest {
    name: String,
    description: Option<String>,
    category_id: Option<Uuid>,
    subcategory_id: Option<Uuid>,
    price: Option<f64>,
    cost_price: Option<f64>,
    #[serde(default)]
    stock_quantity: f64,
    #[serde(default)]
    min_stock: f64,
    #[serde(default)]
    unlimited_stock: bool,
    barcode: Option<String>,
    #[serde(default = "default_unit")]
    unit: String,
    #[serde(default)]
    balance_mode: BalanceMode,
    #[serde(default)]
    image_data: Option<String>,
    #[serde(default)]
    cover_color: Option<String>,
    #[serde(default)]
    availability_schedule: Option<String>,
    #[serde(default)]
    discount_kind: Option<String>,
    #[serde(default)]
    discount_value: Option<f64>,
    #[serde(default)]
    discount_min_qty: Option<f64>,
    #[serde(default)]
    discount_tiers: Option<String>,
    /// IDs dos grupos de adicionais associados (ordem preservada).
    /// `[]` (default) limpa todas as associações no update.
    #[serde(default)]
    addon_group_ids: Vec<Uuid>,
    /// Variações (Fase 5) — JSON array `[{title, selection, required, options}]`.
    #[serde(default)]
    variations: Option<String>,
}

#[derive(Deserialize)]
struct UpdateProductRequest {
    name: String,
    description: Option<String>,
    category_id: Option<Uuid>,
    subcategory_id: Option<Uuid>,
    price: Option<f64>,
    cost_price: Option<f64>,
    #[serde(default)]
    stock_quantity: f64,
    #[serde(default)]
    min_stock: f64,
    #[serde(default)]
    unlimited_stock: bool,
    barcode: Option<String>,
    #[serde(default = "default_unit")]
    unit: String,
    #[serde(default)]
    balance_mode: BalanceMode,
    #[serde(default)]
    image_data: Option<String>,
    #[serde(default)]
    cover_color: Option<String>,
    #[serde(default)]
    availability_schedule: Option<String>,
    #[serde(default)]
    discount_kind: Option<String>,
    #[serde(default)]
    discount_value: Option<f64>,
    #[serde(default)]
    discount_min_qty: Option<f64>,
    #[serde(default)]
    discount_tiers: Option<String>,
    #[serde(default)]
    addon_group_ids: Vec<Uuid>,
    /// Variações (Fase 5) — JSON array `[{title, selection, required, options}]`.
    #[serde(default)]
    variations: Option<String>,
}

fn default_unit() -> String { "un".to_string() }

/// GET /products — lista todos os produtos da empresa.
async fn list(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
) -> Result<Json<Vec<Product>>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("products.view")?;
    let products = state.product_service.find_all(tenant.company_id).await?;
    Ok(Json(products))
}

/// GET /products/:id — busca produto por ID.
async fn get_one(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
) -> Result<Json<Product>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("products.view")?;
    let product = state
        .product_service
        .find_by_id(tenant.company_id, id)
        .await?
        .ok_or_else(|| ServerError::Core(CoreError::NotFound("Product not found".into())))?;

    Ok(Json(product))
}

/// POST /products — cria um novo produto (201 Created).
async fn create(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Json(body): Json<CreateProductRequest>,
) -> Result<(StatusCode, Json<Product>), ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("products.edit")?;
    let product = state
        .product_service
        .create(
            tenant.company_id, body.name, body.description,
            body.category_id, body.subcategory_id, body.price, body.cost_price,
            body.stock_quantity, body.min_stock, body.unlimited_stock, body.barcode, body.unit,
            body.balance_mode, body.image_data, body.cover_color,
            body.availability_schedule,
            body.discount_kind, body.discount_value, body.discount_min_qty,
            body.discount_tiers,
            body.addon_group_ids,
            body.variations,
        )
        .await?;
    Ok((StatusCode::CREATED, Json(product)))
}

/// PUT /products/:id — atualiza um produto existente.
async fn update(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateProductRequest>,
) -> Result<Json<Product>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("products.edit")?;
    let product = state
        .product_service
        .update(
            tenant.company_id, id, body.name, body.description,
            body.category_id, body.subcategory_id, body.price, body.cost_price,
            body.stock_quantity, body.min_stock, body.unlimited_stock, body.barcode, body.unit,
            body.balance_mode, body.image_data, body.cover_color,
            body.availability_schedule,
            body.discount_kind, body.discount_value, body.discount_min_qty,
            body.discount_tiers,
            body.addon_group_ids,
            body.variations,
        )
        .await?;
    Ok(Json(product))
}

/// DELETE /products/:id — remoção lógica (soft delete).
async fn delete(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("products.edit")?;
    state
        .product_service
        .soft_delete(tenant.company_id, id)
        .await?;

    Ok(Json(serde_json::json!({ "deleted": true })))
}
