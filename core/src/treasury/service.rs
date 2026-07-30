use std::sync::Arc;

use chrono::NaiveDateTime;
use rust_decimal::Decimal;
use uuid::Uuid;

use super::model::Treasury;
use super::repository::TreasuryRepository;
use crate::error::CoreError;

/// Serviço da carteira do estabelecimento (tesouraria).
///
/// Regras aplicadas (AI_RULES.md §1, §11, §14):
/// - Toda validação acontece aqui (saldo inicial não-negativo,
///   singleton por empresa) — nunca confiar na UI/REST.
/// - Acesso a dados apenas via [`TreasuryRepository`] (§10).
pub struct TreasuryService {
    repo: Arc<dyn TreasuryRepository>,
}

impl TreasuryService {
    pub fn new(repo: Arc<dyn TreasuryRepository>) -> Self {
        Self { repo }
    }

    /// Carteira da empresa (singleton) — `None` se ainda não criada.
    pub async fn find(&self, company_id: Uuid) -> Result<Option<Treasury>, CoreError> {
        self.repo.find_by_company(company_id).await
    }

    /// Cria a carteira do estabelecimento. Singleton por empresa:
    /// erro de validação se já existir. `initial_balance >= 0`.
    pub async fn open(
        &self,
        company_id: Uuid,
        initial_balance: Decimal,
        notes: Option<String>,
    ) -> Result<Treasury, CoreError> {
        validate_initial_balance(initial_balance)?;
        if self.repo.find_by_company(company_id).await?.is_some() {
            return Err(CoreError::Validation(
                "A carteira do estabelecimento já foi criada".into(),
            ));
        }
        // Normaliza a observação: em branco vira None (sem lixo no banco).
        let notes = notes
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty());
        let treasury = Treasury::new(company_id, initial_balance, notes);
        self.repo.create(&treasury).await?;
        Ok(treasury)
    }

    // ── Sync (delegação + validação company_id) ──

    pub async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Treasury>, CoreError> {
        self.repo.find_unsynced(company_id).await
    }

    pub async fn mark_synced(
        &self,
        company_id: Uuid,
        id: Uuid,
        updated_at: NaiveDateTime,
    ) -> Result<(), CoreError> {
        self.repo.mark_synced(company_id, id, updated_at).await
    }

    /// Upsert vindo do sync. Recebe a entidade por valor para poder
    /// marcar `synced = true` e validar o `company_id` contra o do
    /// chamador (AI_RULES.md §11 — nunca confiar no payload).
    pub async fn sync_upsert(&self, company_id: Uuid, mut t: Treasury) -> Result<(), CoreError> {
        if t.base.company_id != company_id {
            return Err(CoreError::Validation(
                "Operação não permitida para esta empresa".into(),
            ));
        }
        t.base.synced = true;
        self.repo.sync_upsert(&t).await
    }

    pub async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
    ) -> Result<Vec<Treasury>, CoreError> {
        self.repo.find_updated_since(company_id, since).await
    }
}

// ── Validações puras ─────────────────────────────────────────────

fn validate_initial_balance(initial_balance: Decimal) -> Result<(), CoreError> {
    if initial_balance < Decimal::ZERO {
        return Err(CoreError::Validation(
            "O saldo inicial não pode ser negativo".into(),
        ));
    }
    Ok(())
}
