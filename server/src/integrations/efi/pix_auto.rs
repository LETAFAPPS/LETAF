use async_trait::async_trait;
use serde_json::{json, Value};
use uuid::Uuid;

use letaf_core::error::CoreError;
use letaf_core::payment_gateway::pix_auto::{
    CreatedRecurrence, PixAutoChargeEvent, PixAutoGateway, PixAutoInput, RecurrenceStatus,
};

use super::client::EfiClient;

/// Implementação do **Pix Automático** sobre o `EfiClient` (API PIX,
/// mTLS + OAuth já configurados).
///
/// ⚠️ Os caminhos/payloads do Pix Automático (`/v2/rec`, `/v2/cobr`,
/// webhook) seguem o padrão BACEN/Efi mas são novos — **validar em
/// homologação**. Ficam isolados aqui; o corpo de erro é logado.
/// Doc: <https://dev.efipay.com.br/docs/api-pix/pix-automatico>
#[async_trait]
impl PixAutoGateway for EfiClient {
    fn name(&self) -> &str {
        "efi"
    }

    /// `POST /v2/rec` — cria a recorrência (mandato) e devolve o QR de
    /// autorização (copia-e-cola) para o pagador aprovar no banco dele.
    /// Jornada 3 ("cobra a 1ª já"): cria um **cob imediato** (1ª
    /// cobrança, que gera o QR/copia-e-cola) e em seguida a
    /// **recorrência** vinculada a ele via `ativacao.dadosJornada.txid`.
    /// O pagador escaneia o QR do cob → paga a 1ª e autoriza o mandato.
    ///
    /// Schema validado em homologação: `devedor` vai DENTRO de `vinculo`,
    /// sem `recebedor` no corpo; `objeto`/`contrato` ASCII ≤ 35.
    async fn create_recurrence(
        &self,
        input: &PixAutoInput,
    ) -> Result<CreatedRecurrence, CoreError> {
        let token = self.bearer().await?;
        let valor = format!("{:.2}", input.amount_cents as f64 / 100.0);
        let objeto = clean_short(&input.description);

        // 1) cob imediato (1ª cobrança) → QR de autorização.
        let txid = Uuid::new_v4().simple().to_string(); // 32 hex, txid válido
        let cob_url = format!("{}/v2/cob/{}", self.base_url(), txid);
        let cob_body = json!({
            "calendario": { "expiracao": 3600 },
            "valor": { "original": valor },
            "chave": self.pix_key(),
            "solicitacaoPagador": objeto,
            "devedor": { "cpf": input.customer.cpf, "nome": input.customer.name },
        });
        let cob_resp = self
            .http()
            .put(&cob_url)
            .bearer_auth(&token)
            .json(&cob_body)
            .send()
            .await
            .map_err(|e| CoreError::Repository(format!("Efi PUT /v2/cob: {e}")))?;
        let cob = json_or_err(cob_resp, "PUT", "/v2/cob").await?;
        let copia_cola = cob
            .get("pixCopiaECola")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let qr_code_b64 = match cob.get("loc").and_then(|l| l.get("id")).and_then(|i| i.as_i64()) {
            Some(loc_id) => self.fetch_loc_qrcode(&token, loc_id).await.unwrap_or_default(),
            None => String::new(),
        };

        // 2) recorrência (mandato) vinculada ao cob.
        let contrato: String = input
            .custom_id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(35)
            .collect();
        // `dataInicial` NÃO pode ser hoje (data de criação). Como a 1ª
        // cobrança é o cob imediato, a recorrência começa no próximo
        // ciclo: hoje + interval_months.
        let today = chrono::Local::now().date_naive();
        let data_inicial = today
            .checked_add_months(chrono::Months::new(input.interval_months))
            .unwrap_or(today)
            .to_string();
        let rec_body = json!({
            "vinculo": {
                "contrato": contrato,
                "devedor": { "cpf": input.customer.cpf, "nome": input.customer.name },
                "objeto": objeto,
            },
            "calendario": {
                "dataInicial": data_inicial,
                "periodicidade": periodicidade(input.interval_months),
            },
            "valor": { "valorRec": valor },
            "politicaRetentativa": "NAO_PERMITE",
            "ativacao": { "dadosJornada": { "txid": txid } },
        });
        let rec_url = format!("{}/v2/rec", self.base_url());
        let rec_resp = self
            .http()
            .post(&rec_url)
            .bearer_auth(&token)
            .json(&rec_body)
            .send()
            .await
            .map_err(|e| CoreError::Repository(format!("Efi /v2/rec: {e}")))?;
        let v = json_or_err(rec_resp, "POST", "/v2/rec").await?;
        let rec_id = v
            .get("idRec")
            .and_then(|x| x.as_str())
            .ok_or_else(|| CoreError::Repository("Efi rec: idRec ausente".into()))?
            .to_string();
        let status = map_rec_status(v.get("status").and_then(|x| x.as_str()).unwrap_or("CRIADA"));
        Ok(CreatedRecurrence {
            rec_id,
            copia_cola,
            qr_code_b64,
            status,
        })
    }

    /// `GET /v2/rec/{idRec}` — status do mandato.
    async fn fetch_recurrence_status(
        &self,
        rec_id: &str,
    ) -> Result<RecurrenceStatus, CoreError> {
        let token = self.bearer().await?;
        let url = format!("{}/v2/rec/{}", self.base_url(), rec_id);
        let resp = self
            .http()
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| CoreError::Repository(format!("Efi GET /v2/rec: {e}")))?;
        let v = json_or_err(resp, "GET", "/v2/rec").await?;
        Ok(RecurrenceStatus {
            status: map_rec_status(v.get("status").and_then(|x| x.as_str()).unwrap_or("")),
            next_charge_date: v
                .get("calendario")
                .and_then(|c| c.get("dataProximaCobranca").or_else(|| c.get("dataInicial")))
                .and_then(|d| d.as_str())
                .and_then(parse_date),
        })
    }

    /// `POST /v2/cobr` — cobrança recorrente de um ciclo (débito
    /// automático no vencimento).
    async fn create_recurring_charge(
        &self,
        rec_id: &str,
        amount_cents: i64,
        due_date: chrono::NaiveDate,
        description: &str,
        custom_id: &str,
    ) -> Result<(), CoreError> {
        // ⚠️ `cobr` só pode ser criado depois que a recorrência está
        // APROVADA (ciclos 2+). Schema conforme doc; `devedor`/`recebedor`
        // são herdados do `rec`. A validar com pagamento simulado em
        // homologação. `description`/`custom_id` mantidos p/ rastreio.
        let _ = (description, custom_id);
        let token = self.bearer().await?;
        let txid = Uuid::new_v4().simple().to_string();
        let url = format!("{}/v2/cobr/{}", self.base_url(), txid);
        let body = json!({
            "idRec": rec_id,
            "calendario": { "dataDeVencimento": due_date.to_string() },
            "valor": { "original": format!("{:.2}", amount_cents as f64 / 100.0) },
            "ajusteDiaUtil": true,
        });
        let resp = self
            .http()
            .put(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| CoreError::Repository(format!("Efi /v2/cobr: {e}")))?;
        json_or_err(resp, "PUT", "/v2/cobr").await?;
        Ok(())
    }

    /// `PATCH /v2/rec/{idRec}` — cancela o mandato.
    async fn cancel_recurrence(&self, rec_id: &str) -> Result<(), CoreError> {
        let token = self.bearer().await?;
        let url = format!("{}/v2/rec/{}", self.base_url(), rec_id);
        let resp = self
            .http()
            .patch(&url)
            .bearer_auth(&token)
            .json(&json!({ "status": "CANCELADA" }))
            .send()
            .await
            .map_err(|e| CoreError::Repository(format!("Efi PATCH /v2/rec: {e}")))?;
        json_or_err(resp, "PATCH", "/v2/rec").await?;
        Ok(())
    }

    /// O webhook PIX entrega o payload direto (validado por mTLS). Aqui
    /// extraímos os débitos `cobr` liquidados/rejeitados.
    fn parse_webhook(&self, body: &str) -> Result<Vec<PixAutoChargeEvent>, CoreError> {
        let v: Value = serde_json::from_str(body)
            .map_err(|e| CoreError::Repository(format!("Webhook PIX inválido: {e}")))?;
        let arr = v
            .get("cobr")
            .or_else(|| v.get("pix"))
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default();
        Ok(arr.iter().filter_map(parse_cobr_event).collect())
    }
}

impl EfiClient {
    /// `GET /v2/loc/{id}/qrcode` — PNG b64 do QR de autorização.
    async fn fetch_loc_qrcode(&self, token: &str, loc_id: i64) -> Result<String, CoreError> {
        let url = format!("{}/v2/loc/{}/qrcode", self.base_url(), loc_id);
        let resp = self
            .http()
            .get(&url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| CoreError::Repository(format!("Efi qrcode: {e}")))?;
        let v = json_or_err(resp, "GET", "/v2/loc/qrcode").await?;
        Ok(v.get("imagemQrcode")
            .and_then(|x| x.as_str())
            .map(strip_data_url)
            .unwrap_or_default())
    }
}

async fn json_or_err(
    resp: reqwest::Response,
    verb: &str,
    path: &str,
) -> Result<Value, CoreError> {
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        tracing::warn!("Efi Pix Automático {verb} {path} falhou ({status}): {body}");
        return Err(CoreError::Repository(format!(
            "Efi {verb} {path}: status {status}"
        )));
    }
    resp.json::<Value>()
        .await
        .map_err(|e| CoreError::Repository(format!("Efi decode {path}: {e}")))
}

/// Normaliza um item de débito (`cobr`) em evento. Ignora itens sem
/// `idRec`.
fn parse_cobr_event(item: &Value) -> Option<PixAutoChargeEvent> {
    let rec_id = item
        .get("idRec")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())?
        .to_string();
    // Um `cobr` com `horario`/`pix` liquidado conta como pago; status
    // explícito (quando presente) tem prioridade.
    let status = item
        .get("status")
        .and_then(|s| s.as_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_else(|| {
            if item.get("horario").is_some() || item.get("endToEndId").is_some() {
                "paid".into()
            } else {
                "unpaid".into()
            }
        });
    let amount = item
        .get("valor")
        .and_then(|val| val.get("original").or(Some(val)))
        .and_then(|x| x.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);
    let paid_at = item
        .get("horario")
        .and_then(|x| x.as_str())
        .and_then(parse_datetime);
    Some(PixAutoChargeEvent {
        rec_id,
        status,
        amount,
        paid_at,
    })
}

/// Normaliza para ASCII, colapsa espaços e trunca em 35 — limite do
/// `objeto`/`contrato` no schema do Pix Automático (BACEN).
fn clean_short(s: &str) -> String {
    let ascii: String = s
        .chars()
        .map(|c| if c.is_ascii() { c } else { ' ' })
        .collect();
    ascii.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(35).collect()
}

fn periodicidade(months: u32) -> &'static str {
    match months {
        6 => "SEMESTRAL",
        12 => "ANUAL",
        _ => "MENSAL",
    }
}

fn map_rec_status(s: &str) -> String {
    match s {
        "ATIVA" | "APROVADA" => "active".into(),
        "REJEITADA" => "rejected".into(),
        "CANCELADA" | "EXPIRADA" => "canceled".into(),
        _ => "pending".into(),
    }
}

fn strip_data_url(s: &str) -> String {
    s.split_once(',').map(|(_, r)| r.to_string()).unwrap_or_else(|| s.to_string())
}

fn parse_date(s: &str) -> Option<chrono::NaiveDate> {
    let head = s.split(['T', ' ']).next().unwrap_or(s);
    chrono::NaiveDate::parse_from_str(head, "%Y-%m-%d").ok()
}

fn parse_datetime(s: &str) -> Option<chrono::NaiveDateTime> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.naive_utc())
        .or_else(|| parse_date(s).and_then(|d| d.and_hms_opt(12, 0, 0)))
}
