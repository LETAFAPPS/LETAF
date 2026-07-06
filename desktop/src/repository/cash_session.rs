use async_trait::async_trait;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::SqlitePool;
use uuid::Uuid;

use letaf_core::cash::model::{CashSession, SessionStatus};
use letaf_core::cash::repository::CashSessionRepository;
use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;

use super::helpers::{map_db, parse_timestamp, parse_uuid, ts};

#[derive(FromRow)]
struct CashSessionRow {
    id: String,
    company_id: String,
    operator_id: String,
    operator_name: String,
    opened_at: String,
    closed_at: Option<String>,
    initial_change: f64,
    counted_cash: Option<f64>,
    status: String,
    open_notes: Option<String>,
    close_notes: Option<String>,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    synced: bool,
}

impl TryFrom<CashSessionRow> for CashSession {
    type Error = CoreError;
    fn try_from(r: CashSessionRow) -> Result<Self, Self::Error> {
        Ok(Self {
            base: BaseFields {
                id: parse_uuid(&r.id)?,
                company_id: parse_uuid(&r.company_id)?,
                created_at: parse_timestamp(&r.created_at)?,
                updated_at: parse_timestamp(&r.updated_at)?,
                deleted_at: r.deleted_at.as_deref().map(parse_timestamp).transpose()?,
                synced: r.synced,
            },
            operator_id: parse_uuid(&r.operator_id)?,
            operator_name: r.operator_name,
            opened_at: parse_timestamp(&r.opened_at)?,
            closed_at: r.closed_at.as_deref().map(parse_timestamp).transpose()?,
            initial_change: r.initial_change,
            counted_cash: r.counted_cash,
            status: SessionStatus::from_str(&r.status),
            open_notes: r.open_notes,
            close_notes: r.close_notes,
        })
    }
}

pub struct SqliteCashSessionRepository {
    pool: SqlitePool,
}

impl SqliteCashSessionRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CashSessionRepository for SqliteCashSessionRepository {
    async fn find_by_id(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<Option<CashSession>, CoreError> {
        let row = sqlx::query_as::<_, CashSessionRow>(
            "SELECT * FROM cash_sessions WHERE company_id = ? AND id = ? AND deleted_at IS NULL",
        )
        .bind(company_id.to_string())
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        row.map(CashSession::try_from).transpose()
    }

    async fn find_active(&self, company_id: Uuid) -> Result<Option<CashSession>, CoreError> {
        let row = sqlx::query_as::<_, CashSessionRow>(
            "SELECT * FROM cash_sessions
             WHERE company_id = ? AND status = 'open' AND deleted_at IS NULL
             ORDER BY opened_at DESC LIMIT 1",
        )
        .bind(company_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        row.map(CashSession::try_from).transpose()
    }

    async fn find_recent(
        &self,
        company_id: Uuid,
        limit: i64,
    ) -> Result<Vec<CashSession>, CoreError> {
        let rows = sqlx::query_as::<_, CashSessionRow>(
            "SELECT * FROM cash_sessions
             WHERE company_id = ? AND deleted_at IS NULL
             ORDER BY opened_at DESC LIMIT ?",
        )
        .bind(company_id.to_string())
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(CashSession::try_from).collect()
    }

    async fn create(&self, s: &CashSession) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO cash_sessions
             (id, company_id, operator_id, operator_name, opened_at, closed_at,
              initial_change, counted_cash, status, open_notes, close_notes,
              created_at, updated_at, deleted_at, synced)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(s.base.id.to_string())
        .bind(s.base.company_id.to_string())
        .bind(s.operator_id.to_string())
        .bind(&s.operator_name)
        .bind(ts(s.opened_at))
        .bind(s.closed_at.map(ts))
        .bind(s.initial_change)
        .bind(s.counted_cash)
        .bind(s.status.to_string())
        .bind(&s.open_notes)
        .bind(&s.close_notes)
        .bind(ts(s.base.created_at))
        .bind(ts(s.base.updated_at))
        .bind(s.base.deleted_at.map(ts))
        .bind(s.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn update(&self, s: &CashSession) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE cash_sessions SET
               operator_id = ?, operator_name = ?, opened_at = ?, closed_at = ?,
               initial_change = ?, counted_cash = ?, status = ?,
               open_notes = ?, close_notes = ?, updated_at = ?, deleted_at = ?, synced = ?
             WHERE company_id = ? AND id = ?",
        )
        .bind(s.operator_id.to_string())
        .bind(&s.operator_name)
        .bind(ts(s.opened_at))
        .bind(s.closed_at.map(ts))
        .bind(s.initial_change)
        .bind(s.counted_cash)
        .bind(s.status.to_string())
        .bind(&s.open_notes)
        .bind(&s.close_notes)
        .bind(ts(s.base.updated_at))
        .bind(s.base.deleted_at.map(ts))
        .bind(s.base.synced)
        .bind(s.base.company_id.to_string())
        .bind(s.base.id.to_string())
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    // ── Sync ──

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<CashSession>, CoreError> {
        let rows = sqlx::query_as::<_, CashSessionRow>(
            "SELECT * FROM cash_sessions WHERE company_id = ? AND synced = 0",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(CashSession::try_from).collect()
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query("UPDATE cash_sessions SET synced = 1 WHERE company_id = ? AND id = ? AND updated_at = ?")
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
    ) -> Result<Vec<CashSession>, CoreError> {
        let rows = sqlx::query_as::<_, CashSessionRow>(
            "SELECT * FROM cash_sessions WHERE company_id = ? AND updated_at > ?",
        )
        .bind(company_id.to_string())
        .bind(ts(since))
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(CashSession::try_from).collect()
    }

    async fn sync_upsert(&self, s: &CashSession) -> Result<(), CoreError> {
        // SQLite UPSERT com guard de updated_at (last-write-wins).
        sqlx::query(
            "INSERT INTO cash_sessions
             (id, company_id, operator_id, operator_name, opened_at, closed_at,
              initial_change, counted_cash, status, open_notes, close_notes,
              created_at, updated_at, deleted_at, synced)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
               operator_id = excluded.operator_id,
               operator_name = excluded.operator_name,
               opened_at = excluded.opened_at,
               closed_at = excluded.closed_at,
               initial_change = excluded.initial_change,
               counted_cash = excluded.counted_cash,
               status = excluded.status,
               open_notes = excluded.open_notes,
               close_notes = excluded.close_notes,
               updated_at = excluded.updated_at,
               deleted_at = excluded.deleted_at,
               synced = excluded.synced
             WHERE excluded.updated_at > cash_sessions.updated_at",
        )
        .bind(s.base.id.to_string())
        .bind(s.base.company_id.to_string())
        .bind(s.operator_id.to_string())
        .bind(&s.operator_name)
        .bind(ts(s.opened_at))
        .bind(s.closed_at.map(ts))
        .bind(s.initial_change)
        .bind(s.counted_cash)
        .bind(s.status.to_string())
        .bind(&s.open_notes)
        .bind(&s.close_notes)
        .bind(ts(s.base.created_at))
        .bind(ts(s.base.updated_at))
        .bind(s.base.deleted_at.map(ts))
        .bind(s.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }
}
