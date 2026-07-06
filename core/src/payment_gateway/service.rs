use std::sync::Arc;

use uuid::Uuid;

use super::gateway::PaymentGateway;
use super::model::{ChargeStatus, PaymentCharge};
use super::repository::PaymentChargeRepository;
use crate::error::CoreError;

/// Service de cobranças avulsas.
///
/// Regras aplicadas (AI_RULES.md §11):
/// - Orquestra repository + gateway.
/// - Valida tudo antes de chamar o gateway (amount > 0, txid válido).
/// - Falha de gateway é gravada no `last_error` e o registro continua
///   `Pending` para retry manual (não joga fora o trabalho).
pub struct PaymentService {
    repo: Arc<dyn PaymentChargeRepository>,
    gateway: Arc<dyn PaymentGateway>,
}

impl PaymentService {
    pub fn new(
        repo: Arc<dyn PaymentChargeRepository>,
        gateway: Arc<dyn PaymentGateway>,
    ) -> Self {
        Self { repo, gateway }
    }

    /// Cria uma cobrança PIX para pagar uma fatura. Pipeline:
    ///   1. Valida o input.
    ///   2. Persiste rascunho local (status Pending).
    ///   3. Chama o gateway (POST /v2/cob).
    ///   4. Atualiza com txid/QR/copia-cola.
    ///
    /// Se 3 falhar, o rascunho fica para retry — operador vê erro
    /// no `last_error` (não exibido na UI).
    pub async fn create_pix_charge(
        &self,
        company_id: Uuid,
        invoice_id: Option<Uuid>,
        amount: f64,
        description: &str,
    ) -> Result<PaymentCharge, CoreError> {
        if amount <= 0.0 {
            return Err(CoreError::Validation(
                "Valor da cobrança deve ser positivo".into(),
            ));
        }
        let mut charge = PaymentCharge::new_pix(company_id, invoice_id, amount);
        charge.gateway = self.gateway.name().to_string();
        self.repo.create(&charge).await?;

        match self.gateway.create_pix_charge(&charge, description).await {
            Ok(created) => {
                charge.txid = Some(created.txid);
                charge.pix_copia_cola = Some(created.pix_copia_cola);
                charge.qr_code_b64 = Some(created.qr_code_b64);
                charge.expires_at = created.expires_at;
                charge.base.updated_at = chrono::Utc::now().naive_utc();
                charge.base.synced = false;
                self.repo.update(&charge).await?;
                Ok(charge)
            }
            Err(e) => {
                charge.last_error = Some(format!("{e}"));
                charge.base.updated_at = chrono::Utc::now().naive_utc();
                self.repo.update(&charge).await?;
                Err(e)
            }
        }
    }

    /// Consulta o status atual no gateway e atualiza local.
    /// Polling roda no desktop a cada N segundos enquanto o operador
    /// olha o QR Code.
    pub async fn refresh_status(
        &self,
        company_id: Uuid,
        charge_id: Uuid,
    ) -> Result<PaymentCharge, CoreError> {
        let mut charge = self
            .repo
            .find_by_id(company_id, charge_id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Cobrança não encontrada".into()))?;
        let Some(txid) = charge.txid.clone() else {
            return Ok(charge); // ainda não foi criada no gateway
        };
        if charge.status.is_terminal() {
            return Ok(charge);
        }
        let update = self.gateway.fetch_charge_status(&txid).await?;
        charge.status = update.status;
        if matches!(charge.status, ChargeStatus::Paid) {
            charge.paid_at = update.paid_at.or_else(|| Some(chrono::Utc::now().naive_utc()));
        }
        charge.last_error = update.last_error;
        charge.base.updated_at = chrono::Utc::now().naive_utc();
        charge.base.synced = false;
        self.repo.update(&charge).await?;
        Ok(charge)
    }

    pub async fn find_by_id(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<Option<PaymentCharge>, CoreError> {
        self.repo.find_by_id(company_id, id).await
    }
}
