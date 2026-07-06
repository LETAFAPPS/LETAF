//! Conta do cliente: perfil (ver/editar) + histórico de pedidos, via
//! `#[server]` (proxy com Bearer). AI_RULES §11: o cliente só exibe e
//! coleta; o backend valida o token e a senha atual antes de alterar.

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

/// Dados do perfil (`/customer/profile`).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ProfileInfo {
    pub name: String,
    pub email: String,
    pub phone: Option<String>,
}

/// Resumo de pedido para o histórico (`/orders/mine`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderSummary {
    pub number: i64,
    pub status: String,
    pub total: f64,
    pub created_at: String,
}

#[server]
pub async fn get_profile(token: String) -> Result<ProfileInfo, ServerFnError> {
    let host = crate::session::tenant_host().await?;
    crate::api::customer_profile(&host, &token)
        .await
        .map_err(ServerFnError::new)
}

#[server]
pub async fn update_profile(
    token: String,
    name: String,
    phone: String,
    password: String,
    current_password: String,
) -> Result<ProfileInfo, ServerFnError> {
    let host = crate::session::tenant_host().await?;
    crate::api::update_customer_profile(&host, &token, &name, &phone, &password, &current_password)
        .await
        .map_err(ServerFnError::new)
}

#[server]
pub async fn list_orders(token: String) -> Result<Vec<OrderSummary>, ServerFnError> {
    let host = crate::session::tenant_host().await?;
    crate::api::customer_orders(&host, &token)
        .await
        .map_err(ServerFnError::new)
}
