use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use reqwest::{Client, Identity};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::RwLock;

use letaf_core::error::CoreError;
use letaf_core::payment_gateway::gateway::{
    ChargeStatusUpdate, CreatedCharge, PaymentGateway,
};
use letaf_core::payment_gateway::model::{ChargeStatus, PaymentCharge};

use crate::config::EfiConfig;

/// Token OAuth + expiração (Instant para evitar drift de relógio).
#[derive(Clone)]
struct CachedToken {
    access_token: String,
    valid_until: Instant,
}

/// Cliente Efi reutilizável (Arc-clonável). Mantém o `reqwest::Client`
/// configurado com mTLS (Identity .p12) e cache de token OAuth.
pub struct EfiClient {
    http: Client,
    cfg: EfiConfig,
    token: Arc<RwLock<Option<CachedToken>>>,
}

impl EfiClient {
    /// Constrói o cliente carregando o `.p12` do disco e configurando
    /// mTLS no `reqwest`. Falha cedo se o arquivo não existir/senha errada.
    pub fn new(cfg: EfiConfig) -> Result<Self, CoreError> {
        let p12 = std::fs::read(&cfg.p12_path).map_err(|e| {
            CoreError::Repository(format!(
                "EFI_P12_PATH não pôde ser lido ({}): {e}",
                cfg.p12_path
            ))
        })?;
        let identity = Identity::from_pkcs12_der(&p12, &cfg.p12_password).map_err(|e| {
            CoreError::Repository(format!(
                "EFI_P12: identidade inválida (senha incorreta?): {e}"
            ))
        })?;
        let http = Client::builder()
            .identity(identity)
            // Timeouts conservadores — gateway costuma responder < 5s.
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| CoreError::Repository(format!("reqwest build: {e}")))?;
        Ok(Self {
            http,
            cfg,
            token: Arc::new(RwLock::new(None)),
        })
    }

    /// Acesso ao `reqwest::Client` (com mTLS) para os módulos irmãos
    /// da Efi (ex.: Pix Automático), que reutilizam a mesma conexão.
    pub(crate) fn http(&self) -> &Client {
        &self.http
    }

    /// Base da API PIX (`pix.api...` / `pix-h.api...`).
    pub(crate) fn base_url(&self) -> &'static str {
        self.cfg.base_url()
    }

    /// Chave PIX do recebedor (recebe os débitos do Pix Automático).
    pub(crate) fn pix_key(&self) -> &str {
        &self.cfg.pix_key
    }

    pub(crate) async fn bearer(&self) -> Result<String, CoreError> {
        // Caminho rápido: token válido no cache.
        if let Some(t) = self.token.read().await.as_ref() {
            if t.valid_until > Instant::now() {
                return Ok(t.access_token.clone());
            }
        }
        // Renova com lock de escrita — se outra task chegou primeiro,
        // pega o token quente sem refazer a requisição.
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

    async fn fetch_token(&self) -> Result<CachedToken, CoreError> {
        let credentials = format!("{}:{}", self.cfg.client_id, self.cfg.client_secret);
        let auth = format!("Basic {}", B64.encode(credentials));
        let url = format!("{}/oauth/token", self.cfg.base_url());
        let resp = self
            .http
            .post(&url)
            .header("Authorization", auth)
            .json(&json!({ "grant_type": "client_credentials" }))
            .send()
            .await
            .map_err(|e| CoreError::Repository(format!("Efi OAuth: {e}")))?;
        if !resp.status().is_success() {
            let status = resp.status();
            // Body é texto curto da Efi; logamos e devolvemos só o status.
            let body = resp.text().await.unwrap_or_default();
            tracing::warn!("Efi OAuth falhou ({status}): {body}");
            return Err(CoreError::Repository(format!(
                "Efi OAuth: status {status}"
            )));
        }
        let parsed: TokenResponse = resp
            .json()
            .await
            .map_err(|e| CoreError::Repository(format!("Efi OAuth decode: {e}")))?;
        // Renovamos antes do real para não pegar 401 em flight.
        let ttl = Duration::from_secs(parsed.expires_in.saturating_sub(60).max(60));
        Ok(CachedToken {
            access_token: parsed.access_token,
            valid_until: Instant::now() + ttl,
        })
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

#[async_trait]
impl PaymentGateway for EfiClient {
    fn name(&self) -> &str {
        "efi"
    }

    /// `POST /v2/cob` — cria cobrança PIX imediata.
    /// Doc: <https://dev.efipay.com.br/docs/api-pix/cobrancas-imediatas/>
    async fn create_pix_charge(
        &self,
        charge: &PaymentCharge,
        description: &str,
    ) -> Result<CreatedCharge, CoreError> {
        let token = self.bearer().await?;
        let url = format!("{}/v2/cob", self.cfg.base_url());
        // Expiração em segundos — 1h é razoável para "Pagar fatura agora".
        let body = json!({
            "calendario": { "expiracao": 3600 },
            "valor": { "original": format!("{:.2}", charge.amount) },
            "chave": self.cfg.pix_key,
            "solicitacaoPagador": truncate_solicitacao(description),
        });
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| CoreError::Repository(format!("Efi cob: {e}")))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            tracing::warn!("Efi POST /v2/cob falhou ({status}): {body}");
            return Err(CoreError::Repository(format!(
                "Efi cob: status {status}"
            )));
        }
        let parsed: CobResponse = resp
            .json()
            .await
            .map_err(|e| CoreError::Repository(format!("Efi cob decode: {e}")))?;
        // QR Code é gerado separadamente via /v2/loc/:id/qrcode.
        let qrcode = self.fetch_qrcode(&token, parsed.loc.id).await?;
        Ok(CreatedCharge {
            txid: parsed.txid,
            pix_copia_cola: qrcode.qrcode,
            qr_code_b64: strip_data_url(&qrcode.imagem_qrcode),
            expires_at: None, // calendario.expiracao é relativo, não absoluto
        })
    }

    /// `GET /v2/cob/:txid` — status da cobrança.
    async fn fetch_charge_status(
        &self,
        txid: &str,
    ) -> Result<ChargeStatusUpdate, CoreError> {
        let token = self.bearer().await?;
        let url = format!("{}/v2/cob/{}", self.cfg.base_url(), txid);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| CoreError::Repository(format!("Efi cob status: {e}")))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            tracing::warn!("Efi GET /v2/cob/{txid} falhou ({status}): {body}");
            return Err(CoreError::Repository(format!(
                "Efi cob status: status {status}"
            )));
        }
        let parsed: CobStatusResponse = resp
            .json()
            .await
            .map_err(|e| CoreError::Repository(format!("Efi cob status decode: {e}")))?;
        Ok(ChargeStatusUpdate {
            status: map_status(&parsed.status),
            paid_at: parsed
                .pix
                .as_ref()
                .and_then(|v| v.first())
                .and_then(|p| parse_efi_datetime(&p.horario)),
            last_error: None,
        })
    }
}

impl EfiClient {
    /// `GET /v2/loc/:id/qrcode` — devolve o BR Code (copia-cola) e PNG b64.
    async fn fetch_qrcode(&self, token: &str, loc_id: i64) -> Result<QrCodeResponse, CoreError> {
        let url = format!("{}/v2/loc/{}/qrcode", self.cfg.base_url(), loc_id);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| CoreError::Repository(format!("Efi qrcode: {e}")))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            tracing::warn!("Efi GET /v2/loc/{loc_id}/qrcode falhou ({status}): {body}");
            return Err(CoreError::Repository(format!(
                "Efi qrcode: status {status}"
            )));
        }
        resp.json()
            .await
            .map_err(|e| CoreError::Repository(format!("Efi qrcode decode: {e}")))
    }
}

/// `solicitacaoPagador` é limitado a 140 caracteres pela Efi.
fn truncate_solicitacao(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.chars().count() <= 140 {
        trimmed.to_string()
    } else {
        trimmed.chars().take(137).collect::<String>() + "..."
    }
}

/// "data:image/png;base64,iVBORw..." → "iVBORw...".
/// Outras strings já em base64 puro passam intactas.
fn strip_data_url(s: &str) -> String {
    s.split_once(',')
        .map(|(_, rest)| rest.to_string())
        .unwrap_or_else(|| s.to_string())
}

fn map_status(s: &str) -> ChargeStatus {
    match s {
        "CONCLUIDA" => ChargeStatus::Paid,
        "REMOVIDA_PELO_USUARIO_RECEBEDOR" | "REMOVIDA_PELO_PSP" => ChargeStatus::Cancelled,
        _ => ChargeStatus::Pending,
    }
}

/// Efi devolve `horario` como ISO-8601 com timezone. Usamos
/// `DateTime::parse_from_rfc3339` e convertemos para naive UTC.
fn parse_efi_datetime(s: &str) -> Option<chrono::NaiveDateTime> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.naive_utc())
}

#[derive(Deserialize)]
struct CobResponse {
    txid: String,
    loc: CobLoc,
}

#[derive(Deserialize)]
struct CobLoc {
    id: i64,
}

#[derive(Deserialize)]
struct QrCodeResponse {
    qrcode: String,
    #[serde(rename = "imagemQrcode")]
    imagem_qrcode: String,
}

#[derive(Deserialize)]
struct CobStatusResponse {
    status: String,
    pix: Option<Vec<CobPix>>,
}

#[derive(Deserialize)]
struct CobPix {
    horario: String,
}
