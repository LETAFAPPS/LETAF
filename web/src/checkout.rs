//! Checkout: envia o pedido à API via `#[server]` (proxy com Bearer).
//! AI_RULES §11: a UI manda os itens, mas o backend REVALIDA preços e
//! cupom (`verify_item_prices`) — o `unit_price`/total daqui é só
//! ergonomia. O navegador nunca fala direto com a API.

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

/// Item do pedido enviado ao backend (`unit_price` já inclui adicionais).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderItemPayload {
    pub product_id: String,
    pub product_name: String,
    pub quantity: f64,
    pub unit_price: f64,
    pub addons_json: Option<String>,
}

/// Confirmação exibida ao cliente (subconjunto do pedido criado).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderConfirmation {
    pub number: i64,
    pub total: f64,
}

/// POST /orders (proxy, Bearer). `token` vem da sessão do cliente.
#[server]
pub async fn create_order(
    token: String,
    items: Vec<OrderItemPayload>,
    notes: String,
    coupon: String,
) -> Result<OrderConfirmation, ServerFnError> {
    use axum::http::{header::HOST, HeaderMap};
    let headers: HeaderMap = leptos_axum::extract().await?;
    let host = headers
        .get(HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    crate::api::create_order(&host, &token, items, &notes, &coupon)
        .await
        .map_err(ServerFnError::new)
}
