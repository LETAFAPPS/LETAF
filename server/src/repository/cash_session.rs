use async_trait::async_trait;
use rust_decimal::Decimal;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::PgPool;
use uuid::Uuid;

use letaf_core::cash::model::{CashSession, SessionStatus};
use letaf_core::cash::repository::CashSessionRepository;
use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;

use super::helpers::map_db;

#[derive(FromRow)]
struct CashSessionRow {
    id: Uuid,
    company_id: Uuid,
    operator_id: Uuid,
    operator_name: String,
    opened_at: NaiveDateTime,
    closed_at: Option<NaiveDateTime>,
    initial_change: Decimal,
    counted_cash: Option<Decimal>,
    status: String,
    open_notes: Option<String>,
    close_notes: Option<String>,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    deleted_at: Option<NaiveDateTime>,
    synced: bool,
}

impl From<CashSessionRow> for CashSession {
    fn from(r: CashSessionRow) -> Self {
        Self {
            base: BaseFields {
                id: r.id,
                company_id: r.company_id,
                created_at: r.created_at,
                updated_at: r.updated_at,
                deleted_at: r.deleted_at,
                synced: r.synced,
            },
            operator_id: r.operator_id,
            operator_name: r.operator_name,
            opened_at: r.opened_at,
            closed_at: r.closed_at,
            initial_change: r.initial_change,
            counted_cash: r.counted_cash,
            status: SessionStatus::from_str(&r.status),
            open_notes: r.open_notes,
            close_notes: r.close_notes,
        }
    }
}

pub struct PgCashSessionRepository {
    pool: PgPool,
}

impl PgCashSessionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CashSessionRepository for PgCashSessionRepository {
    async fn find_by_id(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<Option<CashSession>, CoreError> {
        Ok(sqlx::query_as::<_, CashSessionRow>(
            "SELECT * FROM cash_sessions WHERE company_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(company_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?
        .map(Into::into))
    }

    async fn find_active(&self, company_id: Uuid) -> Result<Option<CashSession>, CoreError> {
        Ok(sqlx::query_as::<_, CashSessionRow>(
            "SELECT * FROM cash_sessions
             WHERE company_id = $1 AND status = 'open' AND deleted_at IS NULL
             ORDER BY opened_at DESC LIMIT 1",
        )
        .bind(company_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?
        .map(Into::into))
    }

    async fn find_recent(
        &self,
        company_id: Uuid,
        limit: i64,
    ) -> Result<Vec<CashSession>, CoreError> {
        Ok(sqlx::query_as::<_, CashSessionRow>(
            "SELECT * FROM cash_sessions
             WHERE company_id = $1 AND deleted_at IS NULL
             ORDER BY opened_at DESC LIMIT $2",
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

    async fn create(&self, s: &CashSession) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO cash_sessions
             (id, company_id, operator_id, operator_name, opened_at, closed_at,
              initial_change, counted_cash, status, open_notes, close_notes,
              created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)",
        )
        .bind(s.base.id)
        .bind(s.base.company_id)
        .bind(s.operator_id)
        .bind(&s.operator_name)
        .bind(s.opened_at)
        .bind(s.closed_at)
        .bind(s.initial_change)
        .bind(s.counted_cash)
        .bind(s.status.to_string())
        .bind(&s.open_notes)
        .bind(&s.close_notes)
        .bind(s.base.created_at)
        .bind(s.base.updated_at)
        .bind(s.base.deleted_at)
        .bind(s.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn update(&self, s: &CashSession) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE cash_sessions SET
               operator_id = $1, operator_name = $2, opened_at = $3, closed_at = $4,
               initial_change = $5, counted_cash = $6, status = $7,
               open_notes = $8, close_notes = $9, updated_at = $10, deleted_at = $11, synced = $12
             WHERE company_id = $13 AND id = $14",
        )
        .bind(s.operator_id)
        .bind(&s.operator_name)
        .bind(s.opened_at)
        .bind(s.closed_at)
        .bind(s.initial_change)
        .bind(s.counted_cash)
        .bind(s.status.to_string())
        .bind(&s.open_notes)
        .bind(&s.close_notes)
        .bind(s.base.updated_at)
        .bind(s.base.deleted_at)
        .bind(s.base.synced)
        .bind(s.base.company_id)
        .bind(s.base.id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<CashSession>, CoreError> {
        Ok(sqlx::query_as::<_, CashSessionRow>(
            "SELECT * FROM cash_sessions WHERE company_id = $1 AND synced = false",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?
        .into_iter()
        .map(Into::into)
        .collect())
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query("UPDATE cash_sessions SET synced = true WHERE company_id = $1 AND id = $2 AND updated_at = $3")
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
    ) -> Result<Vec<CashSession>, CoreError> {
        Ok(sqlx::query_as::<_, CashSessionRow>(
            "SELECT * FROM cash_sessions WHERE company_id = $1 AND updated_at > $2",
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

    async fn sync_upsert(&self, s: &CashSession) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO cash_sessions
             (id, company_id, operator_id, operator_name, opened_at, closed_at,
              initial_change, counted_cash, status, open_notes, close_notes,
              created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
             ON CONFLICT (id) DO UPDATE SET
               operator_id = EXCLUDED.operator_id,
               operator_name = EXCLUDED.operator_name,
               opened_at = EXCLUDED.opened_at,
               closed_at = EXCLUDED.closed_at,
               initial_change = EXCLUDED.initial_change,
               counted_cash = EXCLUDED.counted_cash,
               status = EXCLUDED.status,
               open_notes = EXCLUDED.open_notes,
               close_notes = EXCLUDED.close_notes,
               updated_at = EXCLUDED.updated_at,
               deleted_at = EXCLUDED.deleted_at,
               synced = EXCLUDED.synced
             WHERE EXCLUDED.updated_at > cash_sessions.updated_at AND cash_sessions.company_id = EXCLUDED.company_id",
        )
        .bind(s.base.id)
        .bind(s.base.company_id)
        .bind(s.operator_id)
        .bind(&s.operator_name)
        .bind(s.opened_at)
        .bind(s.closed_at)
        .bind(s.initial_change)
        .bind(s.counted_cash)
        .bind(s.status.to_string())
        .bind(&s.open_notes)
        .bind(&s.close_notes)
        .bind(s.base.created_at)
        .bind(s.base.updated_at)
        .bind(s.base.deleted_at)
        .bind(s.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }
}
