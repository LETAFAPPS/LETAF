use std::sync::Arc;

use uuid::Uuid;

use super::model::PaymentMethod;
use super::repository::PaymentMethodRepository;
use crate::error::CoreError;

/// Service de formas de pagamento.
///
/// Regras aplicadas (AI_RULES.md §11):
/// - Valida tudo no service (nunca confiar em dados da UI).
/// - Garante 1 default por company (mesmo que o índice já bloqueie).
pub struct PaymentMethodService {
    repo: Arc<dyn PaymentMethodRepository>,
}

impl PaymentMethodService {
    pub fn new(repo: Arc<dyn PaymentMethodRepository>) -> Self {
        Self { repo }
    }

    pub async fn find_by_id(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<Option<PaymentMethod>, CoreError> {
        self.repo.find_by_id(company_id, id).await
    }

    pub async fn find_all(&self, company_id: Uuid) -> Result<Vec<PaymentMethod>, CoreError> {
        self.repo.find_all(company_id).await
    }

    pub async fn find_default(
        &self,
        company_id: Uuid,
    ) -> Result<Option<PaymentMethod>, CoreError> {
        self.repo.find_default(company_id).await
    }

    /// Cria uma nova forma de pagamento. Quando é o 1º método da
    /// company, marca como default automaticamente.
    pub async fn create(
        &self,
        company_id: Uuid,
        kind: String,
        label: String,
        masked: String,
        expiry: String,
        make_default: bool,
    ) -> Result<PaymentMethod, CoreError> {
        validate_input(&kind, &label, &masked, &expiry)?;
        let existing = self.repo.find_all(company_id).await?;
        let mut method = match kind.as_str() {
            "card" => PaymentMethod::new_card(company_id, label, masked, expiry),
            "pix" => PaymentMethod::new_pix(company_id, label),
            // `validate_input` já filtra, mas não dependemos de `unreachable!`
            // (panic mascarado): um refactor futuro retorna erro, não derruba.
            other => {
                return Err(CoreError::Validation(format!(
                    "Tipo de método de pagamento inválido: '{other}'"
                )))
            }
        };
        // Primeiro método sempre vira default; caso contrário respeita
        // a vontade do operador.
        let should_default = existing.is_empty() || make_default;
        if should_default {
            self.repo.clear_default(company_id).await?;
            method.is_default = true;
        }
        self.repo.create(&method).await?;
        Ok(method)
    }

    pub async fn update(
        &self,
        company_id: Uuid,
        id: Uuid,
        label: String,
        masked: String,
        expiry: String,
        make_default: bool,
    ) -> Result<PaymentMethod, CoreError> {
        let mut method = self
            .repo
            .find_by_id(company_id, id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Forma de pagamento não encontrada".into()))?;
        validate_input(&method.kind, &label, &masked, &expiry)?;
        method.label = label;
        method.masked = masked;
        method.expiry = expiry;
        method.base.updated_at = chrono::Utc::now().naive_utc();
        method.base.synced = false;
        if make_default && !method.is_default {
            self.repo.clear_default(company_id).await?;
            method.is_default = true;
        }
        self.repo.update(&method).await?;
        Ok(method)
    }

    /// Define um método existente como padrão. Idempotente.
    pub async fn set_default(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<PaymentMethod, CoreError> {
        let mut method = self
            .repo
            .find_by_id(company_id, id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Forma de pagamento não encontrada".into()))?;
        if method.is_default {
            return Ok(method);
        }
        self.repo.clear_default(company_id).await?;
        method.is_default = true;
        method.base.updated_at = chrono::Utc::now().naive_utc();
        method.base.synced = false;
        self.repo.update(&method).await?;
        Ok(method)
    }

    pub async fn delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let method = self
            .repo
            .find_by_id(company_id, id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Forma de pagamento não encontrada".into()))?;
        // Se era default, promove o próximo método disponível para
        // não deixar a company sem forma padrão.
        let was_default = method.is_default;
        self.repo.soft_delete(company_id, id).await?;
        if was_default {
            if let Some(next) = self
                .repo
                .find_all(company_id)
                .await?
                .into_iter()
                .find(|m| m.base.id != id)
            {
                self.set_default(company_id, next.base.id).await?;
            }
        }
        Ok(())
    }

    // ── Sync (§7) ───────────────────────────────────────────────

    pub async fn find_unsynced(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<PaymentMethod>, CoreError> {
        self.repo.find_unsynced(company_id).await
    }

    pub async fn mark_synced(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        self.repo.mark_synced(company_id, id).await
    }

    pub async fn sync_upsert(
        &self,
        company_id: Uuid,
        mut method: PaymentMethod,
    ) -> Result<(), CoreError> {
        if method.base.company_id != company_id {
            return Err(CoreError::Validation("Company mismatch".into()));
        }
        method.base.synced = true;
        self.repo.sync_upsert(&method).await
    }

    pub async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
    ) -> Result<Vec<PaymentMethod>, CoreError> {
        self.repo.find_updated_since(company_id, since).await
    }
}

/// Validação centralizada — UI nunca confia em si própria (§11).
fn validate_input(
    kind: &str,
    label: &str,
    masked: &str,
    expiry: &str,
) -> Result<(), CoreError> {
    let kind = kind.trim();
    if kind != "card" && kind != "pix" {
        return Err(CoreError::Validation(
            "Tipo de pagamento inválido (use 'card' ou 'pix')".into(),
        ));
    }
    if label.trim().is_empty() {
        return Err(CoreError::Validation("Descrição é obrigatória".into()));
    }
    if kind == "card" {
        let digits: String = masked.chars().filter(|c| c.is_ascii_digit()).collect();
        if digits.len() < 4 {
            return Err(CoreError::Validation(
                "Informe pelo menos os 4 últimos dígitos do cartão".into(),
            ));
        }
        if !is_valid_expiry(expiry) {
            return Err(CoreError::Validation(
                "Validade inválida — use MM/AA".into(),
            ));
        }
    }
    Ok(())
}

/// Aceita "MM/AA" (mês 01-12). Permite "08/28" e "8/28".
fn is_valid_expiry(s: &str) -> bool {
    let s = s.trim();
    let Some((mm, aa)) = s.split_once('/') else {
        return false;
    };
    let mm_ok = mm.chars().all(|c| c.is_ascii_digit())
        && (1..=2).contains(&mm.len())
        && mm.parse::<u8>().map(|m| (1..=12).contains(&m)).unwrap_or(false);
    let aa_ok = aa.chars().all(|c| c.is_ascii_digit()) && aa.len() == 2;
    mm_ok && aa_ok
}
