use axum::extract::{Path, State};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use letaf_core::money;
use axum::http::StatusCode;
use axum::{routing::{get, patch, post}, Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use letaf_core::error::CoreError;
use letaf_core::order::model::{DeliveryType, OrderStatus};
use letaf_core::order::service::OrderItemInput;

use crate::context::AppState;
use crate::error::ServerError;
use crate::jwt::{ROLES_OPERATORS, ROLE_ADMIN, ROLE_CUSTOMER, ROLE_EMPLOYEE};
use crate::middleware::auth::AuthClaims;
use crate::middleware::tenant::TenantContext;

/// Rotas de pedidos.
///
/// Regras aplicadas (AI_RULES.md §1, §11, §12):
/// - POST /orders            → cliente final cria pedido (ROLE_CUSTOMER)
/// - GET  /orders/mine       → cliente final lista seus pedidos (ROLE_CUSTOMER)
/// - GET  /orders            → operador lista pedidos da empresa (ROLES_OPERATORS)
/// - GET  /orders/{id}       → detalhe (customer só vê o seu; user vê qualquer)
/// - PATCH /orders/{id}/status → operador muda status (ROLES_OPERATORS)
/// - Todas as rotas validam tenant via subdomínio (§5, §11)
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/orders", post(create_order).get(list_orders))
        .route("/orders/mine", get(my_orders))
        .route("/orders/{id}", get(get_order))
        .route("/orders/{id}/status", patch(update_status))
        .route("/orders/{id}/cancel", post(cancel_order))
}

#[derive(Deserialize)]
struct CreateOrderRequest {
    items: Vec<OrderItemRequest>,
    /// "delivery" (padrão) ou "pickup".
    #[serde(default)]
    delivery_type: Option<String>,
    notes: Option<String>,
    /// Código do cupom digitado pelo cliente. O servidor revalida e
    /// recalcula o desconto (§11 — nunca confiar no frontend).
    #[serde(default)]
    coupon_code: Option<String>,
}

#[derive(Deserialize)]
struct OrderItemRequest {
    product_id: Uuid,
    product_name: String,
    quantity: f64,
    /// Preço unitário JÁ INCLUINDO os adicionais escolhidos pelo
    /// cliente (a UI calcula). `addons_json` carrega o detalhamento.
    unit_price: Decimal,
    notes: Option<String>,
    /// Snapshot dos adicionais escolhidos (Fase 4): JSON
    /// `[{"name": "...", "price": f64}, ...]`. None quando sem
    /// adicionais.
    #[serde(default)]
    addons_json: Option<String>,
}

#[derive(Serialize)]
struct OrderResponse {
    id: Uuid,
    /// Número sequencial do pedido dentro da empresa (§6, §11).
    number: i64,
    status: String,
    total: f64,
    #[serde(default)]
    discount_amount: f64,
    #[serde(default)]
    additional_amount: f64,
    #[serde(default)]
    coupon_code: Option<String>,
    delivery_type: String,
    notes: Option<String>,
    cancellation_reason: Option<String>,
    created_at: String,
    items: Vec<OrderItemResponse>,
}

#[derive(Serialize)]
struct OrderItemResponse {
    product_id: Uuid,
    product_name: String,
    quantity: f64,
    unit_price: f64,
    subtotal: f64,
}

/// POST /orders — cria um pedido vinculado ao cliente autenticado.
///
/// Regras aplicadas (AI_RULES.md §5, §11):
/// - Exige token de cliente final (`ROLE_CUSTOMER`)
/// - `claims.company_id` deve coincidir com o tenant do subdomínio
async fn create_order(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Json(req): Json<CreateOrderRequest>,
) -> Result<(StatusCode, Json<OrderResponse>), ServerError> {
    auth.verify(tenant.company_id, ROLE_CUSTOMER)?;
    let customer_id = auth.0.sub;

    // §11: o cliente web é não-confiável. Se o lojista fechou a loja
    // manualmente (`store_override == "closed"`), o backend rejeita o pedido
    // — senão uma requisição forjada criaria pedido com a loja fechada. A
    // janela por HORÁRIO não é reforçada aqui porque exigiria o fuso da loja
    // (ausente no modelo); o override manual é independente de fuso.
    let company = state
        .company_service
        .find_by_id(tenant.company_id)
        .await?
        .ok_or_else(|| CoreError::NotFound("Empresa não encontrada".into()))?;
    if company.store_override == "closed" {
        return Err(ServerError::Core(CoreError::Validation(
            "Loja fechada no momento. Tente novamente mais tarde.".into(),
        )));
    }

    let items: Vec<OrderItemInput> = req
        .items
        .into_iter()
        .map(|i| OrderItemInput {
            product_id: i.product_id,
            product_name: i.product_name,
            quantity: i.quantity,
            unit_price: i.unit_price,
            notes: i.notes,
            addons_json: i.addons_json,
        })
        .collect();

    let delivery_type = req.delivery_type
        .as_deref()
        .map(DeliveryType::from_str)
        .unwrap_or_default();

    // ── Cupom (Fase 8) ───────────────────────────────────────────
    // O desconto é SEMPRE recalculado e revalidado no servidor a
    // partir do código (§11). A contagem de uso vem dos próprios
    // pedidos não-cancelados (Order é o registro de uso — sem
    // entidade extra). Pedidos cancelados não contam como uso.
    let (coupon_code, discount_amount) = match req.coupon_code
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        None => (None, rust_decimal::Decimal::ZERO),
        Some(raw_code) => {
            // Arredonda POR ITEM (mesma base de `build_items`): senão o subtotal
            // do cupom diverge do total do pedido por frações de centavo e pode
            // cruzar o limite de `min_order_value`/desconto numa borda.
            let subtotal: Decimal = items
                .iter()
                .map(|i| money::round2(money::qty(i.quantity) * i.unit_price))
                .sum();
            // Contagens via COUNT dedicado (§13) — não materializa o histórico
            // do cliente só para contar (que também tornaria um LIMIT no
            // histórico perigoso para o limite por usuário do cupom).
            let target = raw_code.to_uppercase();
            let customer_prior_orders = state.order_service
                .count_customer_orders(tenant.company_id, customer_id).await?;
            let user_uses = state.order_service
                .count_customer_coupon_uses(tenant.company_id, customer_id, target.as_str()).await?;
            let total_uses = state.order_service
                .count_coupon_uses(tenant.company_id, target.as_str()).await?;
            let now = chrono::Utc::now().naive_utc();
            let (coupon, discount) = state.coupon_service.evaluate(
                tenant.company_id, raw_code, subtotal, now,
                customer_prior_orders, total_uses, user_uses,
            ).await?;
            (Some(coupon.code), discount)
        }
    };

    let order = state
        .order_service
        .create(tenant.company_id, customer_id, items, delivery_type, req.notes,
                coupon_code, discount_amount)
        .await?;

    Ok((StatusCode::CREATED, Json(to_response(&order))))
}

/// GET /orders/mine — lista pedidos do cliente autenticado.
async fn my_orders(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
) -> Result<Json<Vec<OrderResponse>>, ServerError> {
    auth.verify(tenant.company_id, ROLE_CUSTOMER)?;
    let customer_id = auth.0.sub;

    let orders = state
        .order_service
        .find_by_customer(tenant.company_id, customer_id)
        .await?;

    Ok(Json(orders.iter().map(to_response).collect()))
}

/// GET /orders/{id} — detalhe de um pedido.
///
/// Regras aplicadas (AI_RULES.md §11):
/// - `ROLE_CUSTOMER` só pode ler pedidos onde `customer_id == sub`.
/// - `ROLE_ADMIN` e `ROLE_EMPLOYEE` podem ler qualquer pedido da empresa.
async fn get_order(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
) -> Result<Json<OrderResponse>, ServerError> {
    auth.verify_company(tenant.company_id)?;

    let order = state
        .order_service
        .find_by_id(tenant.company_id, id)
        .await?
        .ok_or_else(|| ServerError::Core(CoreError::NotFound("Order not found".into())))?;

    match auth.0.role.as_str() {
        // Operador: precisa da permissão `orders.view` (Admin/SuperAdmin
        // bypassam dentro de `require_permission`). §11 — a autoridade é a
        // permissão concedida pela Função, não o role em si.
        ROLE_ADMIN | ROLE_EMPLOYEE => auth.require_permission("orders.view")?,
        // Cliente final só pode ler o próprio pedido.
        ROLE_CUSTOMER if order.customer_id == auth.0.sub => {}
        _ => {
            return Err(ServerError::Core(CoreError::Unauthorized(
                "Not allowed to read this order".into(),
            )));
        }
    }

    Ok(Json(to_response(&order)))
}

/// GET /orders — lista todos os pedidos da empresa.
///
/// Regras aplicadas (AI_RULES.md §11):
/// - Acesso apenas para operador (`ROLES_OPERATORS`).
/// - Isolamento por `company_id` garantido pelo service.
async fn list_orders(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
) -> Result<Json<Vec<OrderResponse>>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("orders.view")?;

    let orders = state.order_service.find_all(tenant.company_id).await?;
    Ok(Json(orders.iter().map(to_response).collect()))
}

#[derive(Deserialize)]
struct UpdateStatusRequest {
    status: String,
}

/// PATCH /orders/{id}/status — operador muda status do pedido.
///
/// Regras aplicadas (AI_RULES.md §11, §12):
/// - Apenas `ROLES_OPERATORS` pode alterar status.
/// - Validação do valor de status delegada ao `OrderStatus::from_str`.
async fn update_status(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateStatusRequest>,
) -> Result<Json<OrderResponse>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("orders.edit")?;

    let status = OrderStatus::from_str(&req.status)
        .ok_or_else(|| ServerError::Core(CoreError::Validation(format!(
            "Invalid status: {}", req.status
        ))))?;

    let order = state.order_service.update_status(tenant.company_id, id, status).await?;

    Ok(Json(to_response(&order)))
}

#[derive(Deserialize)]
struct CancelRequest {
    reason: String,
}

/// POST /orders/{id}/cancel — operador cancela o pedido informando o motivo.
///
/// Regras aplicadas (AI_RULES.md §6, §11, §12):
/// - Apenas `ROLES_OPERATORS` pode cancelar.
/// - Motivo obrigatório (validado no service).
async fn cancel_order(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
    Json(req): Json<CancelRequest>,
) -> Result<Json<OrderResponse>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("orders.edit")?;

    let order = state.order_service.cancel(tenant.company_id, id, &req.reason).await?;

    Ok(Json(to_response(&order)))
}

fn to_response(order: &letaf_core::order::model::Order) -> OrderResponse {
    OrderResponse {
        id: order.base.id,
        number: order.number,
        status: order.status.to_string(),
        total: order.total.to_f64().unwrap_or(0.0),
        discount_amount: order.discount_amount.to_f64().unwrap_or(0.0),
        additional_amount: order.additional_amount.to_f64().unwrap_or(0.0),
        coupon_code: order.coupon_code.clone(),
        delivery_type: order.delivery_type.to_string(),
        notes: order.notes.clone(),
        cancellation_reason: order.cancellation_reason.clone(),
        created_at: order.base.created_at.to_string(),
        items: order
            .items
            .iter()
            .map(|i| OrderItemResponse {
                product_id: i.product_id,
                product_name: i.product_name.clone(),
                quantity: i.quantity,
                unit_price: i.unit_price.to_f64().unwrap_or(0.0),
                subtotal: i.subtotal.to_f64().unwrap_or(0.0),
            })
            .collect(),
    }
}
