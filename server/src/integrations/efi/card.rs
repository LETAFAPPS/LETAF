use std::sync::Arc;
use rust_decimal::Decimal;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::RwLock;

use letaf_core::error::CoreError;
use letaf_core::payment_gateway::card::{
    CardChargeEvent, CardGateway, CardSubscriptionInput, CardSubscriptionStatus,
    CreatedCardSubscription,
};

use crate::config::EfiCardConfig;

/// Cliente da **API Cobranças** da Efi (cartão recorrente / assinaturas).
///
/// Regras aplicadas (AI_RULES.md §11):
/// - Sem mTLS (diferente do PIX): a API Cobranças autentica só por
///   OAuth `client_credentials` em `/v1/authorize`.
/// - Token cacheado até `expires_at - 60s`.
/// - PAN/CVV NUNCA passam pelo server: o cliente tokeniza via Efi.js e
///   envia só o `payment_token` (fora de escopo PCI).
///
/// ⚠️ Os caminhos/payloads abaixo seguem a doc da API Cobranças da Efi
/// e ficam isolados aqui. Devem ser validados em **homologação** antes
/// de produção (o corpo de erro é logado para facilitar o ajuste).
/// Documentação: <https://dev.efipay.com.br/docs/api-cobrancas/>
pub struct EfiCardClient {
    http: Client,
    cfg: EfiCardConfig,
    token: Arc<RwLock<Option<CachedToken>>>,
}

#[derive(Clone)]
struct CachedToken {
    access_token: String,
    valid_until: Instant,
}

impl EfiCardClient {
    pub fn new(cfg: EfiCardConfig) -> Result<Self, CoreError> {
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| CoreError::Repository(format!("reqwest build (card): {e}")))?;
        Ok(Self {
            http,
            cfg,
            token: Arc::new(RwLock::new(None)),
        })
    }

    async fn bearer(&self) -> Result<String, CoreError> {
        if let Some(t) = self.token.read().await.as_ref() {
            if t.valid_until > Instant::now() {
                return Ok(t.access_token.clone());
            }
        }
        let mut g = self.token.write().await;
        if let Some(t) = g.as_ref() {
            if t.valid_until > Instant::now() {
                return Ok(t.access_token.clone());
            }
        }
        let new = self.fetch_token().await?;
        let bearer = new.access_token.clone();
        *g = Some(new);
        Ok(bearer)
    }

    /// `POST /v1/authorize` — OAuth client_credentials (Basic auth).
    async fn fetch_token(&self) -> Result<CachedToken, CoreError> {
        let credentials = format!("{}:{}", self.cfg.client_id, self.cfg.client_secret);
        let auth = format!("Basic {}", B64.encode(credentials));
        let url = format!("{}/v1/authorize", self.cfg.base_url());
        let resp = self
            .http
            .post(&url)
            .header("Authorization", auth)
            .json(&json!({ "grant_type": "client_credentials" }))
            .send()
            .await
            .map_err(|e| CoreError::Repository(format!("Efi Cobranças OAuth: {e}")))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            tracing::warn!("Efi Cobranças OAuth falhou ({status}): {body}");
            return Err(CoreError::Repository(format!(
                "Efi Cobranças OAuth: status {status}"
            )));
        }
        let parsed: TokenResponse = resp
            .json()
            .await
            .map_err(|e| CoreError::Repository(format!("Efi Cobranças OAuth decode: {e}")))?;
        let ttl = Duration::from_secs(parsed.expires_in.saturating_sub(60).max(60));
        Ok(CachedToken {
            access_token: parsed.access_token,
            valid_until: Instant::now() + ttl,
        })
    }

    /// Wrapper genérico de POST autenticado que devolve o JSON bruto.
    async fn post_json(&self, path: &str, body: Value) -> Result<Value, CoreError> {
        let token = self.bearer().await?;
        let url = format!("{}{}", self.cfg.base_url(), path);
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| CoreError::Repository(format!("Efi Cobranças POST {path}: {e}")))?;
        Self::json_or_err(resp, "POST", path).await
    }

    async fn json_or_err(
        resp: reqwest::Response,
        verb: &str,
        path: &str,
    ) -> Result<Value, CoreError> {
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            tracing::warn!("Efi Cobranças {verb} {path} falhou ({status}): {body}");
            return Err(CoreError::Repository(format!(
                "Efi Cobranças {verb} {path}: status {status}"
            )));
        }
        resp.json::<Value>()
            .await
            .map_err(|e| CoreError::Repository(format!("Efi Cobranças decode {path}: {e}")))
    }

    /// Cria o plano de recorrência. Reaproveitar planos exigiria
    /// guardar o `plan_id`; criamos um por assinatura — simples e a Efi
    /// aceita. `repeats: null` = recorrência indefinida.
    async fn create_plan(&self, name: &str, interval_months: u32) -> Result<i64, CoreError> {
        let body = json!({
            "name": name,
            "interval": interval_months,
            "repeats": Value::Null,
        });
        let v = self.post_json("/v1/plan", body).await?;
        v.get("data")
            .and_then(|d| d.get("plan_id"))
            .and_then(|p| p.as_i64())
            .ok_or_else(|| CoreError::Repository("Efi Cobranças: plan_id ausente".into()))
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

#[async_trait]
impl CardGateway for EfiCardClient {
    fn name(&self) -> &str {
        "efi"
    }

    async fn create_card_subscription(
        &self,
        input: &CardSubscriptionInput,
    ) -> Result<CreatedCardSubscription, CoreError> {
        // 1) Plano de recorrência.
        let plan_id = self.create_plan(&input.plan_name, input.interval_months).await?;

        // 2) Assinatura one-step (cobra a 1ª parcela já com o token).
        let body = json!({
            "items": [{
                "name": input.item_name,
                "value": input.amount_cents,
                "amount": 1,
            }],
            "metadata": {
                "notification_url": input.notification_url,
                "custom_id": input.custom_id,
            },
            "payment": {
                "credit_card": {
                    "customer": {
                        "name": input.customer.name,
                        "cpf": input.customer.cpf,
                        "email": input.customer.email,
                        "phone_number": input.customer.phone,
                        "birth": input.customer.birth,
                    },
                    "installments": 1,
                    "payment_token": input.payment_token,
                    "billing_address": {
                        "street": input.billing_address.street,
                        "number": input.billing_address.number,
                        "neighborhood": input.billing_address.neighborhood,
                        "zipcode": input.billing_address.zipcode,
                        "city": input.billing_address.city,
                        "state": input.billing_address.state,
                    },
                }
            }
        });
        let path = format!("/v1/subscription/{plan_id}/one-step");
        let v = self.post_json(&path, body).await?;
        let data = v
            .get("data")
            .ok_or_else(|| CoreError::Repository("Efi Cobranças: data ausente".into()))?;
        let gateway_subscription_id = data
            .get("subscription_id")
            .map(stringify_id)
            .ok_or_else(|| CoreError::Repository("Efi Cobranças: subscription_id ausente".into()))?;
        let status = data
            .get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("new")
            .to_string();
        let first_charge_status = data
            .get("charge")
            .and_then(|c| c.get("status"))
            .and_then(|s| s.as_str())
            .map(|s| s.to_string());
        Ok(CreatedCardSubscription {
            gateway_subscription_id,
            status,
            card_brand: String::new(),
            card_last4: String::new(),
            next_charge_date: data
                .get("next_execution")
                .and_then(|d| d.as_str())
                .and_then(parse_date),
            first_charge_status,
        })
    }

    async fn fetch_subscription_status(
        &self,
        gateway_subscription_id: &str,
    ) -> Result<CardSubscriptionStatus, CoreError> {
        let token = self.bearer().await?;
        let path = format!("/v1/subscription/{gateway_subscription_id}");
        let url = format!("{}{}", self.cfg.base_url(), path);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| CoreError::Repository(format!("Efi Cobranças GET {path}: {e}")))?;
        let v = Self::json_or_err(resp, "GET", &path).await?;
        let data = v.get("data").unwrap_or(&v);
        Ok(CardSubscriptionStatus {
            status: data
                .get("status")
                .and_then(|s| s.as_str())
                .unwrap_or("unknown")
                .to_string(),
            next_charge_date: data
                .get("next_execution")
                .and_then(|d| d.as_str())
                .and_then(parse_date),
        })
    }

    async fn cancel_subscription(
        &self,
        gateway_subscription_id: &str,
    ) -> Result<(), CoreError> {
        let token = self.bearer().await?;
        let path = format!("/v1/subscription/{gateway_subscription_id}/cancel");
        let url = format!("{}{}", self.cfg.base_url(), path);
        let resp = self
            .http
            .put(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| CoreError::Repository(format!("Efi Cobranças PUT {path}: {e}")))?;
        Self::json_or_err(resp, "PUT", &path).await?;
        Ok(())
    }

    /// `GET /v1/notification/{token}` — detalhes de uma notificação.
    /// A Efi envia só o token no webhook; buscamos os eventos aqui
    /// (autenticados) e devolvemos normalizados.
    async fn fetch_notification(
        &self,
        token: &str,
    ) -> Result<Vec<CardChargeEvent>, CoreError> {
        let bearer = self.bearer().await?;
        let path = format!("/v1/notification/{token}");
        let url = format!("{}{}", self.cfg.base_url(), path);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&bearer)
            .send()
            .await
            .map_err(|e| CoreError::Repository(format!("Efi Cobranças GET {path}: {e}")))?;
        let v = Self::json_or_err(resp, "GET", &path).await?;
        let events = v
            .get("data")
            .and_then(|d| d.as_array())
            .map(|arr| arr.iter().filter_map(parse_notification_event).collect())
            .unwrap_or_default();
        Ok(events)
    }
}

/// O `subscription_id` da Efi pode vir como número ou string.
fn stringify_id(v: &Value) -> String {
    match v {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Normaliza um item de notificação em `CardChargeEvent`. Ignora itens
/// sem `subscription_id` (ex.: eventos não relacionados a assinatura).
fn parse_notification_event(item: &Value) -> Option<CardChargeEvent> {
    let ids = item.get("identifiers");
    let gateway_subscription_id = ids
        .and_then(|i| i.get("subscription_id"))
        .map(stringify_id)
        .filter(|s| !s.is_empty() && s != "null")?;
    // `status` pode ser objeto {current, previous} ou string direta.
    let status = item
        .get("status")
        .and_then(|s| s.get("current").and_then(|c| c.as_str()).or_else(|| s.as_str()))
        .unwrap_or("")
        .to_lowercase();
    let amount = item
        .get("value")
        .and_then(|v| v.as_i64())
        .map(|cents| Decimal::from(cents) / Decimal::from(100))
        .unwrap_or(Decimal::ZERO);
    let paid_at = item
        .get("received_by_bank_at")
        .or_else(|| item.get("paid_at"))
        .and_then(|d| d.as_str())
        .and_then(parse_datetime);
    Some(CardChargeEvent {
        gateway_subscription_id,
        status,
        amount,
        paid_at,
    })
}

/// Aceita "AAAA-MM-DD" (e ignora hora se vier junto).
fn parse_date(s: &str) -> Option<chrono::NaiveDate> {
    let head = s.split(['T', ' ']).next().unwrap_or(s);
    chrono::NaiveDate::parse_from_str(head, "%Y-%m-%d").ok()
}

fn parse_datetime(s: &str) -> Option<chrono::NaiveDateTime> {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(dt.naive_utc());
    }
    parse_date(s).and_then(|d| d.and_hms_opt(12, 0, 0))
}
