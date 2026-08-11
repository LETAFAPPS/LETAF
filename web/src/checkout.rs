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

/// POST /orders (proxy, Bearer). O token vem do cookie HttpOnly (server-side),
/// nunca do cliente (§11).
#[server]
pub async fn create_order(
    items: Vec<OrderItemPayload>,
    notes: String,
    coupon: String,
    /// "delivery" ou "pickup".
    delivery_type: String,
    /// Id do endereço salvo — obrigatório em entrega. O servidor carrega
    /// o endereço do cadastro e confere o dono (§11).
    address_id: String,
) -> Result<OrderConfirmation, ServerFnError> {
    let host = crate::session::tenant_host().await?;
    let token = crate::session::require_token().await?;
    crate::api::create_order(&host, &token, items, &notes, &coupon, &delivery_type, &address_id)
        .await
        .map_err(ServerFnError::new)
}

/// GET /customer/addresses (proxy, Bearer via cookie).
#[server]
pub async fn list_addresses() -> Result<Vec<AddressOption>, ServerFnError> {
    let host = crate::session::tenant_host().await?;
    let token = crate::session::require_token().await?;
    let list = crate::api::customer_addresses(&host, &token)
        .await
        .map_err(ServerFnError::new)?;
    Ok(list
        .into_iter()
        .map(|a| AddressOption {
            resumo: a.resumo(),
            id: a.id,
            label: a.label,
        })
        .collect())
}

/// POST /customer/addresses (proxy, Bearer via cookie).
#[server]
pub async fn add_address(
    label: String,
    street: String,
    number: String,
    neighborhood: String,
    apartment: String,
) -> Result<AddressOption, ServerFnError> {
    let host = crate::session::tenant_host().await?;
    let token = crate::session::require_token().await?;
    let a = crate::api::create_customer_address(
        &host, &token, &label, &street, &number, &neighborhood, &apartment,
    )
    .await
    .map_err(ServerFnError::new)?;
    Ok(AddressOption {
        resumo: a.resumo(),
        id: a.id,
        label: a.label,
    })
}

/// Endereço como o seletor do checkout precisa (id + uma linha legível).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AddressOption {
    pub id: String,
    pub label: String,
    pub resumo: String,
}
