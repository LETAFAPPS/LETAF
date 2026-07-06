//! Tipos do catálogo + busca server-side na API REST (letaf-server).
//!
//! AI_RULES.md §1/§3/§11 (frontend burro): o web NUNCA acessa o banco —
//! só a API. A empresa é resolvida pela API via `Host`; o SSR conecta
//! num endereço interno fixo e encaminha o subdomínio pelo header `Host`.

use serde::{Deserialize, Serialize};

/// `/catalog/info` (subconjunto — serde ignora campos extras).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CatalogInfo {
    pub name: String,
    #[serde(default)]
    pub logo_data: Option<String>,
    #[serde(default)]
    pub cover_data: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
}

/// `/catalog/categories`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CatalogCategory {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub icon_name: Option<String>,
}

/// `/catalog/banners` — banner promocional do topo (subconjunto usado
/// na apresentação). `item_type` "url" abre link externo; outros tipos
/// (ex.: "product") ficam como imagem até o modal de produto existir.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CatalogBanner {
    pub title: String,
    pub image_data: String,
    #[serde(default)]
    pub item_type: String,
    #[serde(default)]
    pub item_url: Option<String>,
}

/// Opção de uma variação (Fase 5): nome + acréscimo de preço.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CatalogVariationOption {
    pub name: String,
    pub price: f64,
}

/// Variação per-produto (ex.: "Tamanho", "Sabor"). `selection` ∈
/// {"single","multi","max_value"}.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CatalogVariation {
    pub title: String,
    pub selection: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub min_select: i64,
    #[serde(default)]
    pub max_select: i64,
    pub options: Vec<CatalogVariationOption>,
}

/// Adicional individual (Fase 4): nome + preço.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CatalogAddon {
    pub id: String,
    pub name: String,
    pub price: f64,
}

/// Grupo de adicionais. `selection` "single" (radio) | "multi".
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CatalogAddonGroup {
    pub id: String,
    pub name: String,
    pub selection: String,
    #[serde(default)]
    pub min_select: i32,
    #[serde(default)]
    pub max_select: i32,
    pub addons: Vec<CatalogAddon>,
}

/// `/catalog/products` — campos usados na apresentação do card.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CatalogProduct {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub price: Option<f64>,
    #[serde(default)]
    pub image_data: Option<String>,
    #[serde(default)]
    pub cover_color: Option<String>,
    #[serde(default)]
    pub category_id: Option<String>,
    #[serde(default)]
    pub discount_kind: Option<String>,
    #[serde(default)]
    pub discount_value: Option<f64>,
    #[serde(default)]
    pub discount_min_qty: Option<f64>,
    #[serde(default)]
    pub discount_tiers: Option<String>,
    /// Janela de disponibilidade por dia da semana (JSON do desktop).
    /// `None`/vazio = sempre disponível.
    #[serde(default)]
    pub availability_schedule: Option<String>,
    /// Grupos de adicionais do produto (alimentam o modal).
    #[serde(default)]
    pub addon_groups: Vec<CatalogAddonGroup>,
    /// Variações per-produto (tamanho/sabor) — já parseadas pela API.
    #[serde(default)]
    pub variations: Vec<CatalogVariation>,
}

/// Um dia no horário de funcionamento da loja (`/catalog/business-hours`).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct BusinessHoursEntry {
    pub day_of_week: i32,
    pub open_time: String,
    pub close_time: String,
    pub is_open: bool,
}

/// Horário de funcionamento + override manual ("open"/"closed"/"none").
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct BusinessHours {
    pub store_override: String,
    pub hours: Vec<BusinessHoursEntry>,
}

/// Payload renderizado pelo SSR (e enviado ao cliente na hidratação).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CatalogData {
    pub info: CatalogInfo,
    pub categories: Vec<CatalogCategory>,
    pub products: Vec<CatalogProduct>,
    pub banners: Vec<CatalogBanner>,
    pub business_hours: BusinessHours,
}

#[cfg(feature = "ssr")]
mod server {
    use super::{
        BusinessHours, CatalogBanner, CatalogCategory, CatalogData, CatalogInfo, CatalogProduct,
    };
    use crate::account::{OrderSummary, ProfileInfo};
    use crate::checkout::{OrderConfirmation, OrderItemPayload};
    use crate::session::SessionInfo;

    /// Endereço interno da API (`LETAF_API_BASE`, padrão `127.0.0.1:3001`).
    /// Separado do tenant: conectamos aqui e encaminhamos a empresa pelo
    /// header `Host` — não dependemos da resolução de `*.localhost`.
    fn api_base() -> String {
        std::env::var("LETAF_API_BASE").unwrap_or_else(|_| "http://127.0.0.1:3001".into())
    }

    /// GET `path` na API com o `Host` do tenant, desserializando o JSON.
    async fn get_json<T: serde::de::DeserializeOwned>(
        client: &reqwest::Client,
        base: &str,
        tenant_host: &str,
        path: &str,
    ) -> Result<T, String> {
        client
            .get(format!("{base}{path}"))
            .header(reqwest::header::HOST, tenant_host)
            .send()
            .await
            .map_err(|e| format!("GET {path}: {e}"))?
            .json::<T>()
            .await
            .map_err(|e| format!("decode {path}: {e}"))
    }

    /// Busca info + categorias + produtos públicos do tenant. `host` é o
    /// `Host` da requisição SSR (ex.: `demo.localhost:3002`); encaminhamos
    /// só o hostname à API, que resolve a empresa pelo subdomínio.
    pub async fn fetch_catalog(host: &str) -> Result<CatalogData, String> {
        let base = api_base();
        let th = host.split(':').next().unwrap_or(host).to_string();
        let client = reqwest::Client::new();
        let info: CatalogInfo = get_json(&client, &base, &th, "/catalog/info").await?;
        let categories: Vec<CatalogCategory> =
            get_json(&client, &base, &th, "/catalog/categories").await?;
        let products: Vec<CatalogProduct> =
            get_json(&client, &base, &th, "/catalog/products").await?;
        // Banners são promocionais (não essenciais): falha aqui não
        // derruba o catálogo — cai numa lista vazia.
        let banners: Vec<CatalogBanner> = get_json(&client, &base, &th, "/catalog/banners")
            .await
            .unwrap_or_default();
        // Horário de funcionamento (não essencial): falha → "none" (sem
        // selo, loja tratada como sempre aberta).
        let business_hours: BusinessHours = get_json(&client, &base, &th, "/catalog/business-hours")
            .await
            .unwrap_or(BusinessHours {
                store_override: "none".into(),
                hours: Vec::new(),
            });
        Ok(CatalogData {
            info,
            categories,
            products,
            banners,
            business_hours,
        })
    }

    /// Extrai uma mensagem amigável do corpo de erro (JSON `{error|message}`)
    /// ou cai num texto por status.
    fn auth_error(status: reqwest::StatusCode, body: &str) -> String {
        if let Some(msg) = serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .and_then(|v| {
                v.get("error")
                    .or_else(|| v.get("message"))
                    .and_then(|m| m.as_str())
                    .map(|s| s.to_string())
            })
        {
            return msg;
        }
        if status == reqwest::StatusCode::UNAUTHORIZED {
            "E-mail ou senha inválidos.".into()
        } else {
            "Não foi possível concluir. Tente novamente.".into()
        }
    }

    /// POST de autenticação: sucesso → `SessionInfo`; erro → mensagem.
    async fn post_auth(
        client: &reqwest::Client,
        base: &str,
        tenant_host: &str,
        path: &str,
        body: serde_json::Value,
    ) -> Result<SessionInfo, String> {
        let resp = client
            .post(format!("{base}{path}"))
            .header(reqwest::header::HOST, tenant_host)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("POST {path}: {e}"))?;
        let status = resp.status();
        if status.is_success() {
            resp.json::<SessionInfo>()
                .await
                .map_err(|e| format!("decode {path}: {e}"))
        } else {
            let text = resp.text().await.unwrap_or_default();
            Err(auth_error(status, &text))
        }
    }

    pub async fn customer_login(
        host: &str,
        email: &str,
        password: &str,
    ) -> Result<SessionInfo, String> {
        let base = api_base();
        let th = host.split(':').next().unwrap_or(host).to_string();
        let client = reqwest::Client::new();
        let body = serde_json::json!({ "email": email, "password": password });
        post_auth(&client, &base, &th, "/customer/login", body).await
    }

    pub async fn customer_register(
        host: &str,
        name: &str,
        email: &str,
        phone: &str,
        password: &str,
    ) -> Result<SessionInfo, String> {
        let base = api_base();
        let th = host.split(':').next().unwrap_or(host).to_string();
        let client = reqwest::Client::new();
        let mut body = serde_json::json!({
            "name": name, "email": email, "password": password,
        });
        if !phone.is_empty() {
            body["phone"] = serde_json::json!(phone);
        }
        post_auth(&client, &base, &th, "/customer/register", body).await
    }

    /// POST /orders com Bearer. Backend revalida preços/cupom (§11).
    pub async fn create_order(
        host: &str,
        token: &str,
        items: Vec<OrderItemPayload>,
        notes: &str,
        coupon: &str,
    ) -> Result<OrderConfirmation, String> {
        let base = api_base();
        let th = host.split(':').next().unwrap_or(host).to_string();
        let client = reqwest::Client::new();

        let items_json: Vec<serde_json::Value> = items
            .iter()
            .map(|i| {
                serde_json::json!({
                    "product_id": i.product_id,
                    "product_name": i.product_name,
                    "quantity": i.quantity,
                    "unit_price": i.unit_price,
                    "addons_json": i.addons_json,
                })
            })
            .collect();
        let mut body = serde_json::json!({ "items": items_json });
        if !notes.is_empty() {
            body["notes"] = serde_json::json!(notes);
        }
        if !coupon.is_empty() {
            body["coupon_code"] = serde_json::json!(coupon);
        }

        let resp = client
            .post(format!("{base}/orders"))
            .header(reqwest::header::HOST, &th)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("POST /orders: {e}"))?;
        let status = resp.status();
        if status.is_success() {
            let v: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| format!("decode /orders: {e}"))?;
            Ok(OrderConfirmation {
                number: v.get("number").and_then(|x| x.as_i64()).unwrap_or(0),
                total: v.get("total").and_then(|x| x.as_f64()).unwrap_or(0.0),
            })
        } else {
            let text = resp.text().await.unwrap_or_default();
            Err(auth_error(status, &text))
        }
    }

    /// GET autenticado (Bearer) genérico, desserializando o JSON.
    async fn get_authed<T: serde::de::DeserializeOwned>(
        client: &reqwest::Client,
        base: &str,
        tenant_host: &str,
        token: &str,
        path: &str,
    ) -> Result<T, String> {
        let resp = client
            .get(format!("{base}{path}"))
            .header(reqwest::header::HOST, tenant_host)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| format!("GET {path}: {e}"))?;
        if resp.status().is_success() {
            resp.json::<T>()
                .await
                .map_err(|e| format!("decode {path}: {e}"))
        } else {
            Err(format!("Falha ao carregar ({}).", resp.status().as_u16()))
        }
    }

    pub async fn customer_profile(host: &str, token: &str) -> Result<ProfileInfo, String> {
        let base = api_base();
        let th = host.split(':').next().unwrap_or(host).to_string();
        let client = reqwest::Client::new();
        get_authed(&client, &base, &th, token, "/customer/profile").await
    }

    pub async fn customer_orders(host: &str, token: &str) -> Result<Vec<OrderSummary>, String> {
        let base = api_base();
        let th = host.split(':').next().unwrap_or(host).to_string();
        let client = reqwest::Client::new();
        get_authed(&client, &base, &th, token, "/orders/mine").await
    }

    pub async fn update_customer_profile(
        host: &str,
        token: &str,
        name: &str,
        phone: &str,
        password: &str,
        current_password: &str,
    ) -> Result<ProfileInfo, String> {
        let base = api_base();
        let th = host.split(':').next().unwrap_or(host).to_string();
        let client = reqwest::Client::new();
        let mut body = serde_json::json!({ "name": name });
        if !phone.is_empty() {
            body["phone"] = serde_json::json!(phone);
        }
        if !password.is_empty() {
            body["password"] = serde_json::json!(password);
            body["current_password"] = serde_json::json!(current_password);
        }
        let resp = client
            .put(format!("{base}/customer/profile"))
            .header(reqwest::header::HOST, &th)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("PUT /customer/profile: {e}"))?;
        let status = resp.status();
        if status.is_success() {
            resp.json::<ProfileInfo>()
                .await
                .map_err(|e| format!("decode profile: {e}"))
        } else {
            let text = resp.text().await.unwrap_or_default();
            Err(auth_error(status, &text))
        }
    }
}

#[cfg(feature = "ssr")]
pub use server::{
    create_order, customer_login, customer_orders, customer_profile, customer_register,
    fetch_catalog, update_customer_profile,
};
