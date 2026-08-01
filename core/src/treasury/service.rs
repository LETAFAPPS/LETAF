use std::sync::Arc;

use chrono::{NaiveDateTime, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

use super::model::{Treasury, TreasuryMovement, TreasuryMovementKind};
use super::repository::TreasuryRepository;
use crate::error::CoreError;
use crate::money::round2;

/// Serviço da carteira do estabelecimento (tesouraria).
///
/// Regras aplicadas (AI_RULES.md §1, §11, §14):
/// - Toda validação acontece aqui (saldo inicial não-negativo, meta de
///   reserva não-negativa, valor de movimento positivo, singleton por
///   empresa) — nunca confiar na UI/REST.
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

    /// Define a meta de reserva mensal do caixa (`>= 0`; `0` = sem meta).
    /// Escrita local marcada `synced = false` — o worker empurra depois
    /// (§7.3: nunca bloquear o usuário esperando rede).
    pub async fn set_reserve_goal(
        &self,
        company_id: Uuid,
        goal: Decimal,
    ) -> Result<Treasury, CoreError> {
        if goal < Decimal::ZERO {
            return Err(CoreError::Validation("A meta não pode ser negativa".into()));
        }
        let mut treasury = self.require_treasury(company_id).await?;
        treasury.reserve_goal = round2(goal);
        treasury.base.updated_at = Utc::now().naive_utc();
        treasury.base.synced = false;
        self.repo.update(&treasury).await?;
        Ok(treasury)
    }

    /// Aporte manual no caixa da empresa.
    pub async fn deposit(
        &self,
        company_id: Uuid,
        amount: Decimal,
        notes: Option<String>,
    ) -> Result<TreasuryMovement, CoreError> {
        self.register_movement(company_id, TreasuryMovementKind::Deposit, amount, notes)
            .await
    }

    /// Retirada manual do caixa da empresa.
    pub async fn withdraw(
        &self,
        company_id: Uuid,
        amount: Decimal,
        notes: Option<String>,
    ) -> Result<TreasuryMovement, CoreError> {
        self.register_movement(company_id, TreasuryMovementKind::Withdraw, amount, notes)
            .await
    }

    /// Movimentos manuais mais recentes primeiro.
    pub async fn find_movements(
        &self,
        company_id: Uuid,
        limit: i64,
    ) -> Result<Vec<TreasuryMovement>, CoreError> {
        self.repo.find_movements(company_id, limit).await
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

    // ── Sync — movimentos manuais ──

    pub async fn find_unsynced_movements(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<TreasuryMovement>, CoreError> {
        self.repo.find_unsynced_movements(company_id).await
    }

    pub async fn mark_movement_synced(
        &self,
        company_id: Uuid,
        id: Uuid,
        updated_at: NaiveDateTime,
    ) -> Result<(), CoreError> {
        self.repo
            .mark_movement_synced(company_id, id, updated_at)
            .await
    }

    /// Upsert de movimento vindo do sync. Recebe por valor para marcar
    /// `synced = true` e validar o tenant do payload (§11).
    pub async fn sync_upsert_movement(
        &self,
        company_id: Uuid,
        mut m: TreasuryMovement,
    ) -> Result<(), CoreError> {
        if m.base.company_id != company_id {
            return Err(CoreError::Validation(
                "Operação não permitida para esta empresa".into(),
            ));
        }
        m.base.synced = true;
        self.repo.sync_upsert_movement(&m).await
    }

    /// Página do pull de movimentos (keyset) — ver o trait.
    pub async fn find_movements_updated_since_paged(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
        after_id: Uuid,
        limit: i64,
    ) -> Result<Vec<TreasuryMovement>, CoreError> {
        self.repo
            .find_movements_updated_since_paged(company_id, since, after_id, limit)
            .await
    }

    pub async fn find_movements_updated_since(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
    ) -> Result<Vec<TreasuryMovement>, CoreError> {
        self.repo
            .find_movements_updated_since(company_id, since)
            .await
    }

    // ── Helpers internos ──

    /// Carteira da empresa ou erro de validação — usada pelas operações
    /// que exigem a carteira já criada.
    async fn require_treasury(&self, company_id: Uuid) -> Result<Treasury, CoreError> {
        self.repo
            .find_by_company(company_id)
            .await?
            .ok_or_else(|| {
                CoreError::Validation("Crie a carteira do estabelecimento primeiro".into())
            })
    }

    /// Núcleo de `deposit`/`withdraw`: valida o valor, exige a carteira
    /// criada e grava o movimento (append-only, `synced = false`).
    async fn register_movement(
        &self,
        company_id: Uuid,
        kind: TreasuryMovementKind,
        amount: Decimal,
        notes: Option<String>,
    ) -> Result<TreasuryMovement, CoreError> {
        if amount <= Decimal::ZERO {
            return Err(CoreError::Validation(
                "Informe um valor maior que zero".into(),
            ));
        }
        let treasury = self.require_treasury(company_id).await?;
        // Normaliza a observação: em branco vira None (sem lixo no banco).
        let notes = notes
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty());
        let movement = TreasuryMovement::new(
            company_id,
            treasury.base.id,
            kind,
            round2(amount),
            notes,
        );
        self.repo.create_movement(&movement).await?;
        Ok(movement)
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
