use std::sync::Arc;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use chrono::Utc;
use uuid::Uuid;

use super::model::{WalletAccount, WalletMovement, WalletMovementKind};
use super::repository::WalletRepository;
use crate::error::CoreError;
use crate::money::round2;

/// Tentativas do controle de concorrência otimista da carteira (§13): sob
/// corrida, a operação perdedora recarrega e retenta este nº de vezes.
const WALLET_MAX_RETRIES: u8 = 5;

/// Serviço da carteira do cliente.
///
/// Regras aplicadas (AI_RULES.md §1, §4, §11, §14):
/// - Validação total aqui: nunca confiar em UI/REST.
/// - Operações de movimento atômicas (`apply_movement` é
///   transacional no repo — §4.Transações).
/// - Limite de fiado verificado em saque/charge.
pub struct WalletService {
    repo: Arc<dyn WalletRepository>,
}

impl WalletService {
    pub fn new(repo: Arc<dyn WalletRepository>) -> Self {
        Self { repo }
    }

    /// Garante que o cliente tem uma conta-carteira. Idempotente:
    /// se já existir, devolve a existente.
    pub async fn open_account(
        &self,
        company_id: Uuid,
        customer_id: Uuid,
        credit_limit: Decimal,
    ) -> Result<WalletAccount, CoreError> {
        validate_credit_limit(credit_limit)?;
        if let Some(existing) = self
            .repo
            .find_account_by_customer(company_id, customer_id)
            .await?
        {
            return Ok(existing);
        }
        let mut account = WalletAccount::new(company_id, customer_id);
        account.credit_limit = credit_limit;
        self.repo.create_account(&account).await?;
        Ok(account)
    }

    /// Atualiza o limite de fiado da conta.
    pub async fn set_credit_limit(
        &self,
        company_id: Uuid,
        account_id: Uuid,
        new_limit: Decimal,
    ) -> Result<WalletAccount, CoreError> {
        validate_credit_limit(new_limit)?;
        let mut account = self.must_load_account(company_id, account_id).await?;
        account.credit_limit = new_limit;
        account.base.updated_at = Utc::now().naive_utc();
        account.base.synced = false;
        self.repo.update_account(&account).await?;
        Ok(account)
    }

    /// Depósito de saldo. `amount > 0`.
    pub async fn deposit(
        &self,
        company_id: Uuid,
        account_id: Uuid,
        amount: Decimal,
        notes: Option<String>,
    ) -> Result<(WalletAccount, WalletMovement), CoreError> {
        validate_positive_amount(amount)?;
        self.apply(
            company_id,
            account_id,
            WalletMovementKind::Deposit,
            amount,
            None,
            notes,
        )
        .await
    }

    /// Saque manual de saldo. Respeita `credit_limit`.
    pub async fn withdraw(
        &self,
        company_id: Uuid,
        account_id: Uuid,
        amount: Decimal,
        notes: Option<String>,
    ) -> Result<(WalletAccount, WalletMovement), CoreError> {
        validate_positive_amount(amount)?;
        self.apply(
            company_id,
            account_id,
            WalletMovementKind::Withdraw,
            amount,
            None,
            notes,
        )
        .await
    }

    /// Cobrança de pedido — consome saldo. Respeita `credit_limit`.
    /// Chamado pelo PDV quando a forma de pagamento é "wallet".
    pub async fn charge_order(
        &self,
        company_id: Uuid,
        account_id: Uuid,
        amount: Decimal,
        order_id: Uuid,
    ) -> Result<(WalletAccount, WalletMovement), CoreError> {
        validate_positive_amount(amount)?;
        self.apply(
            company_id,
            account_id,
            WalletMovementKind::OrderCharge,
            amount,
            Some(order_id),
            None,
        )
        .await
    }

    /// Estorno de cobrança — devolve saldo (cancela uma `OrderCharge`).
    pub async fn refund_order(
        &self,
        company_id: Uuid,
        account_id: Uuid,
        amount: Decimal,
        order_id: Uuid,
    ) -> Result<(WalletAccount, WalletMovement), CoreError> {
        validate_positive_amount(amount)?;
        self.apply(
            company_id,
            account_id,
            WalletMovementKind::OrderRefund,
            amount,
            Some(order_id),
            None,
        )
        .await
    }

    /// Ajuste manual — aceita `amount` negativo para corrigir
    /// inconsistências históricas. Sempre exige `notes` (auditoria).
    pub async fn manual_adjust(
        &self,
        company_id: Uuid,
        account_id: Uuid,
        amount: Decimal,
        notes: String,
    ) -> Result<(WalletAccount, WalletMovement), CoreError> {
        if amount.abs() < dec!(0.005) {
            return Err(CoreError::Validation(
                "Ajuste deve ter valor diferente de zero".into(),
            ));
        }
        if notes.trim().is_empty() {
            return Err(CoreError::Validation(
                "Justificativa obrigatória para ajuste manual".into(),
            ));
        }
        // ManualAdjust não passa por `apply` porque `amount` pode
        // ser negativo (o sinal vai no próprio amount, não no kind).
        // Concorrência otimista com retentativa (§13), igual a `apply`.
        for _ in 0..WALLET_MAX_RETRIES {
            let mut account = self.must_load_account(company_id, account_id).await?;
            let old_balance = account.balance;
            let new_balance = round2(account.balance + amount);
            ensure_within_floor(&account, new_balance)?;
            account.balance = new_balance;
            let now = Utc::now().naive_utc();
            account.base.updated_at = now;
            account.base.synced = false;
            let mut movement = WalletMovement::new(
                company_id,
                account.base.id,
                WalletMovementKind::ManualAdjust,
                amount,
                new_balance,
            );
            movement.notes = Some(notes.clone());
            if self.repo.apply_movement(&account, &movement, old_balance).await? {
                return Ok((account, movement));
            }
        }
        Err(CoreError::Repository(
            "Conflito de concorrência na carteira; tente novamente".into(),
        ))
    }

    // ── Queries ──

    pub async fn find_account_by_id(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<Option<WalletAccount>, CoreError> {
        self.repo.find_account_by_id(company_id, id).await
    }

    pub async fn find_account_by_customer(
        &self,
        company_id: Uuid,
        customer_id: Uuid,
    ) -> Result<Option<WalletAccount>, CoreError> {
        self.repo
            .find_account_by_customer(company_id, customer_id)
            .await
    }

    pub async fn find_all_accounts(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<WalletAccount>, CoreError> {
        self.repo.find_all_accounts(company_id).await
    }

    pub async fn find_movements(
        &self,
        company_id: Uuid,
        account_id: Uuid,
        limit: i64,
    ) -> Result<Vec<WalletMovement>, CoreError> {
        self.repo
            .find_movements_by_account(company_id, account_id, limit)
            .await
    }

    // ── Sync (delegação + validação company_id) ──

    pub async fn find_unsynced_accounts(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<WalletAccount>, CoreError> {
        self.repo.find_unsynced_accounts(company_id).await
    }

    pub async fn mark_account_synced(
        &self,
        company_id: Uuid,
        id: Uuid,
        updated_at: chrono::NaiveDateTime,
    ) -> Result<(), CoreError> {
        self.repo.mark_account_synced(company_id, id, updated_at).await
    }

    pub async fn find_accounts_updated_since(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
    ) -> Result<Vec<WalletAccount>, CoreError> {
        self.repo
            .find_accounts_updated_since(company_id, since)
            .await
    }

    /// Página do pull de carteiras por keyset `(updated_at, id)`.
    pub async fn find_accounts_updated_since_paged(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
        after_id: Uuid,
        limit: i64,
    ) -> Result<Vec<WalletAccount>, CoreError> {
        self.repo
            .find_accounts_updated_since_paged(company_id, since, after_id, limit)
            .await
    }

    pub async fn sync_upsert_account(
        &self,
        company_id: Uuid,
        mut account: WalletAccount,
    ) -> Result<(), CoreError> {
        if account.base.company_id != company_id {
            return Err(CoreError::Validation("Company mismatch".into()));
        }
        account.base.synced = true;
        self.repo.sync_upsert_account(&account).await
    }

    pub async fn find_unsynced_movements(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<WalletMovement>, CoreError> {
        self.repo.find_unsynced_movements(company_id).await
    }

    pub async fn mark_movement_synced(
        &self,
        company_id: Uuid,
        id: Uuid,
        updated_at: chrono::NaiveDateTime,
    ) -> Result<(), CoreError> {
        self.repo.mark_movement_synced(company_id, id, updated_at).await
    }

    pub async fn find_movements_updated_since(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
    ) -> Result<Vec<WalletMovement>, CoreError> {
        self.repo
            .find_movements_updated_since(company_id, since)
            .await
    }

    /// Página do pull de movimentos por keyset `(updated_at, id)`.
    pub async fn find_movements_updated_since_paged(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
        after_id: Uuid,
        limit: i64,
    ) -> Result<Vec<WalletMovement>, CoreError> {
        self.repo
            .find_movements_updated_since_paged(company_id, since, after_id, limit)
            .await
    }

    pub async fn sync_upsert_movement(
        &self,
        company_id: Uuid,
        mut movement: WalletMovement,
    ) -> Result<(), CoreError> {
        if movement.base.company_id != company_id {
            return Err(CoreError::Validation("Company mismatch".into()));
        }
        movement.base.synced = true;
        self.repo.sync_upsert_movement(&movement).await
    }

    // ── Helpers internos ──

    /// Núcleo das operações que somam/subtraem um valor positivo no
    /// balance. Usa o `sign` do `kind` para decidir direção e
    /// valida limite de fiado antes de aplicar.
    async fn apply(
        &self,
        company_id: Uuid,
        account_id: Uuid,
        kind: WalletMovementKind,
        amount: Decimal,
        order_id: Option<Uuid>,
        notes: Option<String>,
    ) -> Result<(WalletAccount, WalletMovement), CoreError> {
        // Concorrência otimista com retentativa (§13): recarrega o saldo,
        // recalcula e só grava se o saldo não mudou desde a leitura. Sob
        // corrida (duplo-clique), a operação perdedora recarrega e retenta.
        for _ in 0..WALLET_MAX_RETRIES {
            let mut account = self.must_load_account(company_id, account_id).await?;
            let old_balance = account.balance;
            let delta = amount * kind.sign();
            let new_balance = round2(account.balance + delta);
            ensure_within_floor(&account, new_balance)?;
            account.balance = new_balance;
            let now = Utc::now().naive_utc();
            account.base.updated_at = now;
            account.base.synced = false;
            let mut movement = WalletMovement::new(
                company_id,
                account.base.id,
                kind,
                amount,
                new_balance,
            );
            movement.related_order_id = order_id;
            movement.notes = notes.clone();
            if self.repo.apply_movement(&account, &movement, old_balance).await? {
                return Ok((account, movement));
            }
        }
        Err(CoreError::Repository(
            "Conflito de concorrência na carteira; tente novamente".into(),
        ))
    }

    async fn must_load_account(
        &self,
        company_id: Uuid,
        account_id: Uuid,
    ) -> Result<WalletAccount, CoreError> {
        self.repo
            .find_account_by_id(company_id, account_id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Carteira não encontrada".into()))
    }
}

// ── Validações puras ─────────────────────────────────────────────

fn validate_positive_amount(amount: Decimal) -> Result<(), CoreError> {
    if amount <= Decimal::ZERO {
        return Err(CoreError::Validation(
            "Valor deve ser maior que zero".into(),
        ));
    }
    Ok(())
}

fn validate_credit_limit(limit: Decimal) -> Result<(), CoreError> {
    if limit < Decimal::ZERO {
        return Err(CoreError::Validation(
            "Limite de fiado deve ser zero ou positivo".into(),
        ));
    }
    Ok(())
}

fn ensure_within_floor(account: &WalletAccount, new_balance: Decimal) -> Result<(), CoreError> {
    if new_balance < account.floor() - dec!(0.005) {
        return Err(CoreError::Validation(format!(
            "Saldo insuficiente — limite de fiado é R$ {}",
            crate::money::round2(account.credit_limit)
        )));
    }
    Ok(())
}
