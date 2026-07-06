use async_trait::async_trait;
use rust_decimal::prelude::ToPrimitive;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::SqlitePool;
use uuid::Uuid;

use letaf_core::cash::model::{CashMovement, MovementKind};
use letaf_core::cash::repository::CashMovementRepository;
use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;

use super::helpers::{map_db, parse_timestamp, parse_uuid, ts};

#[derive(FromRow)]
struct CashMovementRow {
    id: String,
    company_id: String,
    session_id: String,
    kind: String,
    amount: f64,
    method: Option<String>,
    reason: String,
    detail: Option<String>,
    order_id: Option<String>,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    synced: bool,
}

impl TryFrom<CashMovementRow> for CashMovement {
    type Error = CoreError;
    fn try_from(r: CashMovementRow) -> Result<Self, Self::Error> {
        Ok(Self {
            base: BaseFields {
                id: parse_uuid(&r.id)?,
                company_id: parse_uuid(&r.company_id)?,
                created_at: parse_timestamp(&r.created_at)?,
                updated_at: parse_timestamp(&r.updated_at)?,
                deleted_at: r.deleted_at.as_deref().map(parse_timestamp).transpose()?,
                synced: r.synced,
            },
            session_id: parse_uuid(&r.session_id)?,
            kind: MovementKind::from_str(&r.kind),
            amount: letaf_core::money::from_db_f64(r.amount),
            method: r.method,
            reason: r.reason,
            detail: r.detail,
            order_id: r.order_id.as_deref().map(parse_uuid).transpose()?,
        })
    }
}

pub struct SqliteCashMovementRepository {
    pool: SqlitePool,
}

impl SqliteCashMovementRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CashMovementRepository for SqliteCashMovementRepository {
    async fn find_by_id(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<Option<CashMovement>, CoreError> {
        let row = sqlx::query_as::<_, CashMovementRow>(
            "SELECT * FROM cash_movements WHERE company_id = ? AND id = ? AND deleted_at IS NULL",
        )
        .bind(company_id.to_string())
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        row.map(CashMovement::try_from).transpose()
    }

    async fn find_by_session(
        &self,
        company_id: Uuid,
        session_id: Uuid,
    ) -> Result<Vec<CashMovement>, CoreError> {
        let rows = sqlx::query_as::<_, CashMovementRow>(
            "SELECT * FROM cash_movements
             WHERE company_id = ? AND session_id = ? AND deleted_at IS NULL
             ORDER BY created_at ASC",
        )
        .bind(company_id.to_string())
        .bind(session_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(CashMovement::try_from).collect()
    }

    async fn create(&self, m: &CashMovement) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO cash_movements
             (id, company_id, session_id, kind, amount, method, reason, detail,
              order_id, created_at, updated_at, deleted_at, synced)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(m.base.id.to_string())
        .bind(m.base.company_id.to_string())
        .bind(m.session_id.to_string())
        .bind(m.kind.to_string())
        .bind(m.amount.to_f64().unwrap_or(0.0))
        .bind(&m.method)
        .bind(&m.reason)
        .bind(&m.detail)
        .bind(m.order_id.map(|u| u.to_string()))
        .bind(ts(m.base.created_at))
        .bind(ts(m.base.updated_at))
        .bind(m.base.deleted_at.map(ts))
        .bind(m.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    // ── Sync ──

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<CashMovement>, CoreError> {
        let rows = sqlx::query_as::<_, CashMovementRow>(
            "SELECT * FROM cash_movements WHERE company_id = ? AND synced = 0",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(CashMovement::try_from).collect()
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query("UPDATE cash_movements SET synced = 1 WHERE company_id = ? AND id = ? AND updated_at = ?")
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
    ) -> Result<Vec<CashMovement>, CoreError> {
        let rows = sqlx::query_as::<_, CashMovementRow>(
            "SELECT * FROM cash_movements WHERE company_id = ? AND updated_at > ?",
        )
        .bind(company_id.to_string())
        .bind(ts(since))
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(CashMovement::try_from).collect()
    }

    async fn sync_upsert(&self, m: &CashMovement) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO cash_movements
             (id, company_id, session_id, kind, amount, method, reason, detail,
              order_id, created_at, updated_at, deleted_at, synced)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
               kind = excluded.kind,
               amount = excluded.amount,
               method = excluded.method,
               reason = excluded.reason,
               detail = excluded.detail,
               order_id = excluded.order_id,
               updated_at = excluded.updated_at,
               deleted_at = excluded.deleted_at,
               synced = excluded.synced
             WHERE excluded.updated_at > cash_movements.updated_at",
        )
        .bind(m.base.id.to_string())
        .bind(m.base.company_id.to_string())
        .bind(m.session_id.to_string())
        .bind(m.kind.to_string())
        .bind(m.amount.to_f64().unwrap_or(0.0))
        .bind(&m.method)
        .bind(&m.reason)
        .bind(&m.detail)
        .bind(m.order_id.map(|u| u.to_string()))
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
