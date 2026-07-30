use async_trait::async_trait;
use chrono::NaiveDateTime;
use rust_decimal::prelude::ToPrimitive;
use sqlx::prelude::FromRow;
use sqlx::SqlitePool;
use uuid::Uuid;

use letaf_core::error::CoreError;
use letaf_core::treasury::model::Treasury;
use letaf_core::treasury::repository::TreasuryRepository;

use super::helpers::{map_db, parse_base, ts};

// ── Row ──────────────────────────────────────────────────────────

#[derive(FromRow)]
struct TreasuryRow {
    id: String,
    company_id: String,
    initial_balance: f64,
    notes: Option<String>,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    synced: bool,
}

impl TryFrom<TreasuryRow> for Treasury {
    type Error = CoreError;
    fn try_from(r: TreasuryRow) -> Result<Self, Self::Error> {
        Ok(Self {
            base: parse_base(&r.id, &r.company_id, &r.created_at, &r.updated_at, r.deleted_at.as_deref(), r.synced)?,
            // Dinheiro no SQLite é REAL — round-trip via from_db_f64 (§13).
            initial_balance: letaf_core::money::from_db_f64(r.initial_balance),
            notes: r.notes,
        })
    }
}

// ── Repository ───────────────────────────────────────────────────

/// Carteira do estabelecimento no cache local (SQLite).
///
/// Regras aplicadas (AI_RULES.md §5, §10):
/// - Toda query filtra por `company_id` (multi-tenant).
/// - `mark_synced` condicional ao `updated_at` (contrato do sync §7.6).
/// - `sync_upsert` com last-write-wins por `updated_at` (§7.7).
pub struct SqliteTreasuryRepository {
    pool: SqlitePool,
}

impl SqliteTreasuryRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TreasuryRepository for SqliteTreasuryRepository {
    async fn find_by_company(&self, company_id: Uuid) -> Result<Option<Treasury>, CoreError> {
        let row = sqlx::query_as::<_, TreasuryRow>(
            "SELECT * FROM treasury_accounts
             WHERE company_id = ? AND deleted_at IS NULL",
        )
        .bind(company_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        row.map(Treasury::try_from).transpose()
    }

    async fn create(&self, t: &Treasury) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO treasury_accounts
             (id, company_id, initial_balance, notes,
              created_at, updated_at, deleted_at, synced)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(t.base.id.to_string())
        .bind(t.base.company_id.to_string())
        .bind(t.initial_balance.to_f64().unwrap_or(0.0))
        .bind(&t.notes)
        .bind(ts(t.base.created_at))
        .bind(ts(t.base.updated_at))
        .bind(t.base.deleted_at.map(ts))
        .bind(t.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    // ── Sync ──

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Treasury>, CoreError> {
        let rows = sqlx::query_as::<_, TreasuryRow>(
            "SELECT * FROM treasury_accounts WHERE company_id = ? AND synced = 0",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(Treasury::try_from).collect()
    }

    async fn mark_synced(
        &self,
        company_id: Uuid,
        id: Uuid,
        updated_at: NaiveDateTime,
    ) -> Result<(), CoreError> {
        // Condicional ao `updated_at` enviado (§7.6): se o registro mudou
        // durante o push, 0 linhas afetadas → reenvia no próximo ciclo.
        sqlx::query("UPDATE treasury_accounts SET synced = 1 WHERE company_id = ? AND id = ? AND updated_at = ?")
            .bind(company_id.to_string())
            .bind(id.to_string())
            .bind(ts(updated_at))
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
        let rows = sqlx::query_as::<_, TreasuryRow>(
            "SELECT * FROM treasury_accounts WHERE company_id = ? AND updated_at > ?",
        )
        .bind(company_id.to_string())
        .bind(ts(since))
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(Treasury::try_from).collect()
    }

    async fn sync_upsert(&self, t: &Treasury) -> Result<(), CoreError> {
        // Last-write-wins por `updated_at` (§7.7), mesmo padrão das demais
        // entidades sincronizadas.
        sqlx::query(
            "INSERT INTO treasury_accounts
             (id, company_id, initial_balance, notes,
              created_at, updated_at, deleted_at, synced)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
               initial_balance = excluded.initial_balance,
               notes = excluded.notes,
               updated_at = excluded.updated_at,
               deleted_at = excluded.deleted_at,
               synced = excluded.synced
             WHERE excluded.updated_at > treasury_accounts.updated_at",
        )
        .bind(t.base.id.to_string())
        .bind(t.base.company_id.to_string())
        .bind(t.initial_balance.to_f64().unwrap_or(0.0))
        .bind(&t.notes)
        .bind(ts(t.base.created_at))
        .bind(ts(t.base.updated_at))
        .bind(t.base.deleted_at.map(ts))
        .bind(t.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }
}
