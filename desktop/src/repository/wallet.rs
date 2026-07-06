use async_trait::async_trait;
use chrono::{NaiveDateTime, Utc};
use sqlx::prelude::FromRow;
use sqlx::SqlitePool;
use uuid::Uuid;

use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;
use letaf_core::wallet::model::{WalletAccount, WalletMovement, WalletMovementKind};
use letaf_core::wallet::repository::WalletRepository;

use super::helpers::{map_db, parse_timestamp, parse_uuid, ts};

// ── Rows ─────────────────────────────────────────────────────────

#[derive(FromRow)]
struct WalletAccountRow {
    id: String,
    company_id: String,
    customer_id: String,
    balance: f64,
    credit_limit: f64,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    synced: bool,
}

impl TryFrom<WalletAccountRow> for WalletAccount {
    type Error = CoreError;
    fn try_from(r: WalletAccountRow) -> Result<Self, Self::Error> {
        Ok(Self {
            base: BaseFields {
                id: parse_uuid(&r.id)?,
                company_id: parse_uuid(&r.company_id)?,
                created_at: parse_timestamp(&r.created_at)?,
                updated_at: parse_timestamp(&r.updated_at)?,
                deleted_at: r.deleted_at.as_deref().map(parse_timestamp).transpose()?,
                synced: r.synced,
            },
            customer_id: parse_uuid(&r.customer_id)?,
            balance: r.balance,
            credit_limit: r.credit_limit,
        })
    }
}

#[derive(FromRow)]
struct WalletMovementRow {
    id: String,
    company_id: String,
    account_id: String,
    kind: String,
    amount: f64,
    balance_after: f64,
    related_order_id: Option<String>,
    notes: Option<String>,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    synced: bool,
}

impl TryFrom<WalletMovementRow> for WalletMovement {
    type Error = CoreError;
    fn try_from(r: WalletMovementRow) -> Result<Self, Self::Error> {
        Ok(Self {
            base: BaseFields {
                id: parse_uuid(&r.id)?,
                company_id: parse_uuid(&r.company_id)?,
                created_at: parse_timestamp(&r.created_at)?,
                updated_at: parse_timestamp(&r.updated_at)?,
                deleted_at: r.deleted_at.as_deref().map(parse_timestamp).transpose()?,
                synced: r.synced,
            },
            account_id: parse_uuid(&r.account_id)?,
            kind: WalletMovementKind::from_str(&r.kind),
            amount: r.amount,
            balance_after: r.balance_after,
            related_order_id: r.related_order_id.as_deref().map(parse_uuid).transpose()?,
            notes: r.notes,
        })
    }
}

// ── Repository ───────────────────────────────────────────────────

pub struct SqliteWalletRepository {
    pool: SqlitePool,
}

impl SqliteWalletRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl WalletRepository for SqliteWalletRepository {
    async fn find_account_by_id(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<Option<WalletAccount>, CoreError> {
        let row = sqlx::query_as::<_, WalletAccountRow>(
            "SELECT * FROM wallet_accounts
             WHERE company_id = ? AND id = ? AND deleted_at IS NULL",
        )
        .bind(company_id.to_string())
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        row.map(WalletAccount::try_from).transpose()
    }

    async fn find_account_by_customer(
        &self,
        company_id: Uuid,
        customer_id: Uuid,
    ) -> Result<Option<WalletAccount>, CoreError> {
        let row = sqlx::query_as::<_, WalletAccountRow>(
            "SELECT * FROM wallet_accounts
             WHERE company_id = ? AND customer_id = ? AND deleted_at IS NULL",
        )
        .bind(company_id.to_string())
        .bind(customer_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        row.map(WalletAccount::try_from).transpose()
    }

    async fn find_all_accounts(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<WalletAccount>, CoreError> {
        let rows = sqlx::query_as::<_, WalletAccountRow>(
            "SELECT * FROM wallet_accounts
             WHERE company_id = ? AND deleted_at IS NULL",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(WalletAccount::try_from).collect()
    }

    async fn create_account(&self, a: &WalletAccount) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO wallet_accounts
             (id, company_id, customer_id, balance, credit_limit,
              created_at, updated_at, deleted_at, synced)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(a.base.id.to_string())
        .bind(a.base.company_id.to_string())
        .bind(a.customer_id.to_string())
        .bind(a.balance)
        .bind(a.credit_limit)
        .bind(ts(a.base.created_at))
        .bind(ts(a.base.updated_at))
        .bind(a.base.deleted_at.map(ts))
        .bind(a.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn update_account(&self, a: &WalletAccount) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE wallet_accounts SET
               customer_id = ?, balance = ?, credit_limit = ?,
               updated_at = ?, deleted_at = ?, synced = ?
             WHERE company_id = ? AND id = ?",
        )
        .bind(a.customer_id.to_string())
        .bind(a.balance)
        .bind(a.credit_limit)
        .bind(ts(a.base.updated_at))
        .bind(a.base.deleted_at.map(ts))
        .bind(a.base.synced)
        .bind(a.base.company_id.to_string())
        .bind(a.base.id.to_string())
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
        // AI_RULES §4.Transações — balance + movement no MESMO BEGIN.
        // Falha em qualquer step → rollback completo.
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        sqlx::query(
            "UPDATE wallet_accounts SET
               balance = ?, updated_at = ?, synced = ?
             WHERE company_id = ? AND id = ?",
        )
        .bind(a.balance)
        .bind(ts(a.base.updated_at))
        .bind(a.base.synced)
        .bind(a.base.company_id.to_string())
        .bind(a.base.id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(map_db)?;
        sqlx::query(
            "INSERT INTO wallet_movements
             (id, company_id, account_id, kind, amount, balance_after,
              related_order_id, notes,
              created_at, updated_at, deleted_at, synced)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(m.base.id.to_string())
        .bind(m.base.company_id.to_string())
        .bind(m.account_id.to_string())
        .bind(m.kind.to_string())
        .bind(m.amount)
        .bind(m.balance_after)
        .bind(m.related_order_id.map(|u| u.to_string()))
        .bind(&m.notes)
        .bind(ts(m.base.created_at))
        .bind(ts(m.base.updated_at))
        .bind(m.base.deleted_at.map(ts))
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
        let rows = sqlx::query_as::<_, WalletMovementRow>(
            "SELECT * FROM wallet_movements
             WHERE company_id = ? AND account_id = ? AND deleted_at IS NULL
             ORDER BY created_at DESC LIMIT ?",
        )
        .bind(company_id.to_string())
        .bind(account_id.to_string())
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(WalletMovement::try_from).collect()
    }

    // ── Sync — accounts ──

    async fn find_unsynced_accounts(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<WalletAccount>, CoreError> {
        let rows = sqlx::query_as::<_, WalletAccountRow>(
            "SELECT * FROM wallet_accounts WHERE company_id = ? AND synced = 0",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(WalletAccount::try_from).collect()
    }

    async fn mark_account_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query("UPDATE wallet_accounts SET synced = 1 WHERE company_id = ? AND id = ? AND updated_at = ?")
            .bind(company_id.to_string())
            .bind(id.to_string())
            .bind(ts(updated_at))
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
        let rows = sqlx::query_as::<_, WalletAccountRow>(
            "SELECT * FROM wallet_accounts WHERE company_id = ? AND updated_at > ?",
        )
        .bind(company_id.to_string())
        .bind(ts(since))
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(WalletAccount::try_from).collect()
    }

    async fn sync_upsert_account(&self, a: &WalletAccount) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO wallet_accounts
             (id, company_id, customer_id, balance, credit_limit,
              created_at, updated_at, deleted_at, synced)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
               customer_id = excluded.customer_id,
               balance = excluded.balance,
               credit_limit = excluded.credit_limit,
               updated_at = excluded.updated_at,
               deleted_at = excluded.deleted_at,
               synced = excluded.synced
             WHERE excluded.updated_at > wallet_accounts.updated_at",
        )
        .bind(a.base.id.to_string())
        .bind(a.base.company_id.to_string())
        .bind(a.customer_id.to_string())
        .bind(a.balance)
        .bind(a.credit_limit)
        .bind(ts(a.base.created_at))
        .bind(ts(a.base.updated_at))
        .bind(a.base.deleted_at.map(ts))
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
        let rows = sqlx::query_as::<_, WalletMovementRow>(
            "SELECT * FROM wallet_movements WHERE company_id = ? AND synced = 0",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(WalletMovement::try_from).collect()
    }

    async fn mark_movement_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query("UPDATE wallet_movements SET synced = 1 WHERE company_id = ? AND id = ? AND updated_at = ?")
            .bind(company_id.to_string())
            .bind(id.to_string())
            .bind(ts(updated_at))
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
        let rows = sqlx::query_as::<_, WalletMovementRow>(
            "SELECT * FROM wallet_movements WHERE company_id = ? AND updated_at > ?",
        )
        .bind(company_id.to_string())
        .bind(ts(since))
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(WalletMovement::try_from).collect()
    }

    async fn sync_upsert_movement(&self, m: &WalletMovement) -> Result<(), CoreError> {
        // Suprime UPDATE atual via guard — same as cash_movements.
        // Movimentos são append-only, mas `synced` muda.
        let _ = Utc::now();
        sqlx::query(
            "INSERT INTO wallet_movements
             (id, company_id, account_id, kind, amount, balance_after,
              related_order_id, notes,
              created_at, updated_at, deleted_at, synced)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
               kind = excluded.kind,
               amount = excluded.amount,
               balance_after = excluded.balance_after,
               related_order_id = excluded.related_order_id,
               notes = excluded.notes,
               updated_at = excluded.updated_at,
               deleted_at = excluded.deleted_at,
               synced = excluded.synced
             WHERE excluded.updated_at > wallet_movements.updated_at",
        )
        .bind(m.base.id.to_string())
        .bind(m.base.company_id.to_string())
        .bind(m.account_id.to_string())
        .bind(m.kind.to_string())
        .bind(m.amount)
        .bind(m.balance_after)
        .bind(m.related_order_id.map(|u| u.to_string()))
        .bind(&m.notes)
        .bind(ts(m.base.created_at))
        .bind(ts(m.base.updated_at))
        .bind(m.base.deleted_at.map(ts))
        .bind(m.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }
}
