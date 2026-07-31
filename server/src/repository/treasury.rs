use async_trait::async_trait;
use chrono::NaiveDateTime;
use rust_decimal::Decimal;
use sqlx::prelude::FromRow;
use sqlx::PgPool;
use uuid::Uuid;

use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;
use letaf_core::treasury::model::{Treasury, TreasuryMovement, TreasuryMovementKind};
use letaf_core::treasury::repository::TreasuryRepository;

use super::helpers::map_db;

#[derive(FromRow)]
struct TreasuryRow {
    id: Uuid,
    company_id: Uuid,
    initial_balance: Decimal,
    notes: Option<String>,
    reserve_goal: Decimal,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    deleted_at: Option<NaiveDateTime>,
    synced: bool,
}

impl From<TreasuryRow> for Treasury {
    fn from(r: TreasuryRow) -> Self {
        Self {
            base: BaseFields {
                id: r.id,
                company_id: r.company_id,
                created_at: r.created_at,
                updated_at: r.updated_at,
                deleted_at: r.deleted_at,
                synced: r.synced,
            },
            initial_balance: r.initial_balance,
            notes: r.notes,
            reserve_goal: r.reserve_goal,
        }
    }
}

#[derive(FromRow)]
struct TreasuryMovementRow {
    id: Uuid,
    company_id: Uuid,
    treasury_id: Uuid,
    kind: String,
    amount: Decimal,
    notes: Option<String>,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    deleted_at: Option<NaiveDateTime>,
    synced: bool,
}

impl From<TreasuryMovementRow> for TreasuryMovement {
    fn from(r: TreasuryMovementRow) -> Self {
        Self {
            base: BaseFields {
                id: r.id,
                company_id: r.company_id,
                created_at: r.created_at,
                updated_at: r.updated_at,
                deleted_at: r.deleted_at,
                synced: r.synced,
            },
            treasury_id: r.treasury_id,
            kind: TreasuryMovementKind::from_str(&r.kind),
            amount: r.amount,
            notes: r.notes,
        }
    }
}

/// Carteira do estabelecimento no PostgreSQL (dinheiro em NUMERIC, exato).
///
/// Regras aplicadas (AI_RULES.md §5, §10, §11):
/// - Toda query filtra por `company_id` (multi-tenant).
/// - `sync_upsert` com last-write-wins por `updated_at` (§7.7), sem
///   permitir troca de tenant no conflito.
pub struct PgTreasuryRepository {
    pool: PgPool,
}

impl PgTreasuryRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TreasuryRepository for PgTreasuryRepository {
    async fn find_by_company(&self, company_id: Uuid) -> Result<Option<Treasury>, CoreError> {
        Ok(sqlx::query_as::<_, TreasuryRow>(
            "SELECT * FROM treasury_accounts
             WHERE company_id = $1 AND deleted_at IS NULL",
        )
        .bind(company_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?
        .map(Into::into))
    }

    async fn create(&self, t: &Treasury) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO treasury_accounts
             (id, company_id, initial_balance, notes, reserve_goal,
              created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(t.base.id)
        .bind(t.base.company_id)
        .bind(t.initial_balance)
        .bind(&t.notes)
        .bind(t.reserve_goal)
        .bind(t.base.created_at)
        .bind(t.base.updated_at)
        .bind(t.base.deleted_at)
        .bind(t.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn update(&self, t: &Treasury) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE treasury_accounts SET
               initial_balance = $1, notes = $2, reserve_goal = $3,
               updated_at = $4, deleted_at = $5, synced = $6
             WHERE company_id = $7 AND id = $8",
        )
        .bind(t.initial_balance)
        .bind(&t.notes)
        .bind(t.reserve_goal)
        .bind(t.base.updated_at)
        .bind(t.base.deleted_at)
        .bind(t.base.synced)
        .bind(t.base.company_id)
        .bind(t.base.id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    // ── Movimentos manuais ──

    async fn create_movement(&self, m: &TreasuryMovement) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO treasury_movements
             (id, company_id, treasury_id, kind, amount, notes,
              created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(m.base.id)
        .bind(m.base.company_id)
        .bind(m.treasury_id)
        .bind(m.kind.to_string())
        .bind(m.amount)
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

    async fn find_movements(
        &self,
        company_id: Uuid,
        limit: i64,
    ) -> Result<Vec<TreasuryMovement>, CoreError> {
        Ok(sqlx::query_as::<_, TreasuryMovementRow>(
            "SELECT * FROM treasury_movements
             WHERE company_id = $1 AND deleted_at IS NULL
             ORDER BY created_at DESC LIMIT $2",
        )
        .bind(company_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?
        .into_iter()
        .map(Into::into)
        .collect())
    }

    // ── Sync ──

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Treasury>, CoreError> {
        Ok(sqlx::query_as::<_, TreasuryRow>(
            "SELECT * FROM treasury_accounts WHERE company_id = $1 AND synced = FALSE",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?
        .into_iter()
        .map(Into::into)
        .collect())
    }

    async fn mark_synced(
        &self,
        company_id: Uuid,
        id: Uuid,
        updated_at: NaiveDateTime,
    ) -> Result<(), CoreError> {
        // Condicional ao `updated_at` (§7.6) — contrato do sync worker.
        sqlx::query(
            "UPDATE treasury_accounts SET synced = TRUE WHERE company_id = $1 AND id = $2 AND updated_at = $3",
        )
        .bind(company_id)
        .bind(id)
        .bind(updated_at)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
    ) -> Result<Vec<Treasury>, CoreError> {
        Ok(sqlx::query_as::<_, TreasuryRow>(
            "SELECT * FROM treasury_accounts WHERE company_id = $1 AND updated_at > $2",
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

    async fn sync_upsert(&self, t: &Treasury) -> Result<(), CoreError> {
        // Last-write-wins por `updated_at` (§7.7); o guard de `company_id`
        // impede que um conflito de id troque o tenant da linha (§11).
        sqlx::query(
            "INSERT INTO treasury_accounts
             (id, company_id, initial_balance, notes, reserve_goal,
              created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (id) DO UPDATE SET
               initial_balance = EXCLUDED.initial_balance,
               notes = EXCLUDED.notes,
               reserve_goal = EXCLUDED.reserve_goal,
               updated_at = EXCLUDED.updated_at,
               deleted_at = EXCLUDED.deleted_at,
               synced = EXCLUDED.synced
             WHERE EXCLUDED.updated_at > treasury_accounts.updated_at
               AND treasury_accounts.company_id = EXCLUDED.company_id",
        )
        .bind(t.base.id)
        .bind(t.base.company_id)
        .bind(t.initial_balance)
        .bind(&t.notes)
        .bind(t.reserve_goal)
        .bind(t.base.created_at)
        .bind(t.base.updated_at)
        .bind(t.base.deleted_at)
        .bind(t.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    // ── Sync — movimentos ──

    async fn find_unsynced_movements(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<TreasuryMovement>, CoreError> {
        Ok(sqlx::query_as::<_, TreasuryMovementRow>(
            "SELECT * FROM treasury_movements WHERE company_id = $1 AND synced = FALSE",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?
        .into_iter()
        .map(Into::into)
        .collect())
    }

    async fn mark_movement_synced(
        &self,
        company_id: Uuid,
        id: Uuid,
        updated_at: NaiveDateTime,
    ) -> Result<(), CoreError> {
        // Condicional ao `updated_at` (§7.6) — contrato do sync worker.
        sqlx::query(
            "UPDATE treasury_movements SET synced = TRUE WHERE company_id = $1 AND id = $2 AND updated_at = $3",
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
    ) -> Result<Vec<TreasuryMovement>, CoreError> {
        Ok(sqlx::query_as::<_, TreasuryMovementRow>(
            "SELECT * FROM treasury_movements WHERE company_id = $1 AND updated_at > $2",
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

    async fn sync_upsert_movement(&self, m: &TreasuryMovement) -> Result<(), CoreError> {
        // Last-write-wins por `updated_at` (§7.7); o guard de `company_id`
        // impede que um conflito de id troque o tenant da linha (§11).
        sqlx::query(
            "INSERT INTO treasury_movements
             (id, company_id, treasury_id, kind, amount, notes,
              created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (id) DO UPDATE SET
               treasury_id = EXCLUDED.treasury_id,
               kind = EXCLUDED.kind,
               amount = EXCLUDED.amount,
               notes = EXCLUDED.notes,
               updated_at = EXCLUDED.updated_at,
               deleted_at = EXCLUDED.deleted_at,
               synced = EXCLUDED.synced
             WHERE EXCLUDED.updated_at > treasury_movements.updated_at
               AND treasury_movements.company_id = EXCLUDED.company_id",
        )
        .bind(m.base.id)
        .bind(m.base.company_id)
        .bind(m.treasury_id)
        .bind(m.kind.to_string())
        .bind(m.amount)
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
