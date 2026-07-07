use async_trait::async_trait;
use rust_decimal::Decimal;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::PgPool;
use uuid::Uuid;

use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;
use letaf_core::wallet::model::{WalletAccount, WalletMovement, WalletMovementKind};
use letaf_core::wallet::repository::WalletRepository;

use super::helpers::map_db;

#[derive(FromRow)]
struct WalletAccountRow {
    id: Uuid,
    company_id: Uuid,
    customer_id: Uuid,
    balance: Decimal,
    credit_limit: Decimal,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    deleted_at: Option<NaiveDateTime>,
    synced: bool,
}

impl From<WalletAccountRow> for WalletAccount {
    fn from(r: WalletAccountRow) -> Self {
        Self {
            base: BaseFields {
                id: r.id,
                company_id: r.company_id,
                created_at: r.created_at,
                updated_at: r.updated_at,
                deleted_at: r.deleted_at,
                synced: r.synced,
            },
            customer_id: r.customer_id,
            balance: r.balance,
            credit_limit: r.credit_limit,
        }
    }
}

#[derive(FromRow)]
struct WalletMovementRow {
    id: Uuid,
    company_id: Uuid,
    account_id: Uuid,
    kind: String,
    amount: Decimal,
    balance_after: Decimal,
    related_order_id: Option<Uuid>,
    notes: Option<String>,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    deleted_at: Option<NaiveDateTime>,
    synced: bool,
}

impl From<WalletMovementRow> for WalletMovement {
    fn from(r: WalletMovementRow) -> Self {
        Self {
            base: BaseFields {
                id: r.id,
                company_id: r.company_id,
                created_at: r.created_at,
                updated_at: r.updated_at,
                deleted_at: r.deleted_at,
                synced: r.synced,
            },
            account_id: r.account_id,
            kind: WalletMovementKind::from_str(&r.kind),
            amount: r.amount,
            balance_after: r.balance_after,
            related_order_id: r.related_order_id,
            notes: r.notes,
        }
    }
}

pub struct PgWalletRepository {
    pool: PgPool,
}

impl PgWalletRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl WalletRepository for PgWalletRepository {
    async fn find_account_by_id(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<Option<WalletAccount>, CoreError> {
        Ok(sqlx::query_as::<_, WalletAccountRow>(
            "SELECT * FROM wallet_accounts
             WHERE company_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(company_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?
        .map(Into::into))
    }

    async fn find_account_by_customer(
        &self,
        company_id: Uuid,
        customer_id: Uuid,
    ) -> Result<Option<WalletAccount>, CoreError> {
        Ok(sqlx::query_as::<_, WalletAccountRow>(
            "SELECT * FROM wallet_accounts
             WHERE company_id = $1 AND customer_id = $2 AND deleted_at IS NULL",
        )
        .bind(company_id)
        .bind(customer_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?
        .map(Into::into))
    }

    async fn find_all_accounts(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<WalletAccount>, CoreError> {
        Ok(sqlx::query_as::<_, WalletAccountRow>(
            "SELECT * FROM wallet_accounts
             WHERE company_id = $1 AND deleted_at IS NULL",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?
        .into_iter()
        .map(Into::into)
        .collect())
    }

    async fn create_account(&self, a: &WalletAccount) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO wallet_accounts
             (id, company_id, customer_id, balance, credit_limit,
              created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(a.base.id)
        .bind(a.base.company_id)
        .bind(a.customer_id)
        .bind(a.balance)
        .bind(a.credit_limit)
        .bind(a.base.created_at)
        .bind(a.base.updated_at)
        .bind(a.base.deleted_at)
        .bind(a.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn update_account(&self, a: &WalletAccount) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE wallet_accounts SET
               customer_id = $1, balance = $2, credit_limit = $3,
               updated_at = $4, deleted_at = $5, synced = $6
             WHERE company_id = $7 AND id = $8",
        )
        .bind(a.customer_id)
        .bind(a.balance)
        .bind(a.credit_limit)
        .bind(a.base.updated_at)
        .bind(a.base.deleted_at)
        .bind(a.base.synced)
        .bind(a.base.company_id)
        .bind(a.base.id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn apply_movement(
        &self,
        a: &WalletAccount,
        m: &WalletMovement,
    ) -> Result<(), CoreError> {
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        sqlx::query(
            "UPDATE wallet_accounts SET
               balance = $1, updated_at = $2, synced = $3
             WHERE company_id = $4 AND id = $5",
        )
        .bind(a.balance)
        .bind(a.base.updated_at)
        .bind(a.base.synced)
        .bind(a.base.company_id)
        .bind(a.base.id)
        .execute(&mut *tx)
        .await
        .map_err(map_db)?;
        sqlx::query(
            "INSERT INTO wallet_movements
             (id, company_id, account_id, kind, amount, balance_after,
              related_order_id, notes,
              created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
        )
        .bind(m.base.id)
        .bind(m.base.company_id)
        .bind(m.account_id)
        .bind(m.kind.to_string())
        .bind(m.amount)
        .bind(m.balance_after)
        .bind(m.related_order_id)
        .bind(&m.notes)
        .bind(m.base.created_at)
        .bind(m.base.updated_at)
        .bind(m.base.deleted_at)
        .bind(m.base.synced)
        .execute(&mut *tx)
        .await
        .map_err(map_db)?;
        tx.commit().await.map_err(map_db)?;
        Ok(())
    }

    async fn find_movements_by_account(
        &self,
        company_id: Uuid,
        account_id: Uuid,
        limit: i64,
    ) -> Result<Vec<WalletMovement>, CoreError> {
        Ok(sqlx::query_as::<_, WalletMovementRow>(
            "SELECT * FROM wallet_movements
             WHERE company_id = $1 AND account_id = $2 AND deleted_at IS NULL
             ORDER BY created_at DESC LIMIT $3",
        )
        .bind(company_id)
        .bind(account_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?
        .into_iter()
        .map(Into::into)
        .collect())
    }

    // ── Sync — accounts ──

    async fn find_unsynced_accounts(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<WalletAccount>, CoreError> {
        Ok(sqlx::query_as::<_, WalletAccountRow>(
            "SELECT * FROM wallet_accounts WHERE company_id = $1 AND synced = FALSE",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?
        .into_iter()
        .map(Into::into)
        .collect())
    }

    async fn mark_account_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query("UPDATE wallet_accounts SET synced = TRUE WHERE company_id = $1 AND id = $2 AND updated_at = $3")
            .bind(company_id)
            .bind(id)
        .bind(updated_at)
            .execute(&self.pool)
            .await
            .map_err(map_db)?;
        Ok(())
    }

    async fn find_accounts_updated_since(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
    ) -> Result<Vec<WalletAccount>, CoreError> {
        Ok(sqlx::query_as::<_, WalletAccountRow>(
            "SELECT * FROM wallet_accounts WHERE company_id = $1 AND updated_at > $2",
        )
        .bind(company_id)
        .bind(since)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?
        .into_iter()
        .map(Into::into)
        .collect())
    }

    async fn sync_upsert_account(&self, a: &WalletAccount) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO wallet_accounts
             (id, company_id, customer_id, balance, credit_limit,
              created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (id) DO UPDATE SET
               customer_id = EXCLUDED.customer_id,
               balance = EXCLUDED.balance,
               credit_limit = EXCLUDED.credit_limit,
               updated_at = EXCLUDED.updated_at,
               deleted_at = EXCLUDED.deleted_at,
               synced = EXCLUDED.synced
             WHERE EXCLUDED.updated_at > wallet_accounts.updated_at AND wallet_accounts.company_id = EXCLUDED.company_id",
        )
        .bind(a.base.id)
        .bind(a.base.company_id)
        .bind(a.customer_id)
        .bind(a.balance)
        .bind(a.credit_limit)
        .bind(a.base.created_at)
        .bind(a.base.updated_at)
        .bind(a.base.deleted_at)
        .bind(a.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    // ── Sync — movements ──

    async fn find_unsynced_movements(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<WalletMovement>, CoreError> {
        Ok(sqlx::query_as::<_, WalletMovementRow>(
            "SELECT * FROM wallet_movements WHERE company_id = $1 AND synced = FALSE",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?
        .into_iter()
        .map(Into::into)
        .collect())
    }

    async fn mark_movement_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE wallet_movements SET synced = TRUE WHERE company_id = $1 AND id = $2 AND updated_at = $3",
        )
        .bind(company_id)
        .bind(id)
        .bind(updated_at)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn find_movements_updated_since(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
    ) -> Result<Vec<WalletMovement>, CoreError> {
        Ok(sqlx::query_as::<_, WalletMovementRow>(
            "SELECT * FROM wallet_movements WHERE company_id = $1 AND updated_at > $2",
        )
        .bind(company_id)
        .bind(since)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?
        .into_iter()
        .map(Into::into)
        .collect())
    }

    async fn find_movements_updated_since_paged(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
        after_id: Uuid,
        limit: i64,
    ) -> Result<Vec<WalletMovement>, CoreError> {
        Ok(sqlx::query_as::<_, WalletMovementRow>(
            "SELECT * FROM wallet_movements
              WHERE company_id = $1
                AND (updated_at > $2 OR (updated_at = $2 AND id > $3))
              ORDER BY updated_at ASC, id ASC
              LIMIT $4",
        )
        .bind(company_id)
        .bind(since)
        .bind(after_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?
        .into_iter()
        .map(Into::into)
        .collect())
    }

    async fn sync_upsert_movement(&self, m: &WalletMovement) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO wallet_movements
             (id, company_id, account_id, kind, amount, balance_after,
              related_order_id, notes,
              created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
             ON CONFLICT (id) DO UPDATE SET
               kind = EXCLUDED.kind,
               amount = EXCLUDED.amount,
               balance_after = EXCLUDED.balance_after,
               related_order_id = EXCLUDED.related_order_id,
               notes = EXCLUDED.notes,
               updated_at = EXCLUDED.updated_at,
               deleted_at = EXCLUDED.deleted_at,
               synced = EXCLUDED.synced
             WHERE EXCLUDED.updated_at > wallet_movements.updated_at AND wallet_movements.company_id = EXCLUDED.company_id",
        )
        .bind(m.base.id)
        .bind(m.base.company_id)
        .bind(m.account_id)
        .bind(m.kind.to_string())
        .bind(m.amount)
        .bind(m.balance_after)
        .bind(m.related_order_id)
        .bind(&m.notes)
        .bind(m.base.created_at)
        .bind(m.base.updated_at)
        .bind(m.base.deleted_at)
        .bind(m.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }
}
