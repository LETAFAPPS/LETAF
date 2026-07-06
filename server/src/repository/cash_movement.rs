use async_trait::async_trait;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::PgPool;
use uuid::Uuid;

use letaf_core::cash::model::{CashMovement, MovementKind};
use letaf_core::cash::repository::CashMovementRepository;
use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;

use super::helpers::map_db;

#[derive(FromRow)]
struct CashMovementRow {
    id: Uuid,
    company_id: Uuid,
    session_id: Uuid,
    kind: String,
    amount: f64,
    method: Option<String>,
    reason: String,
    detail: Option<String>,
    order_id: Option<Uuid>,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    deleted_at: Option<NaiveDateTime>,
    synced: bool,
}

impl From<CashMovementRow> for CashMovement {
    fn from(r: CashMovementRow) -> Self {
        Self {
            base: BaseFields {
                id: r.id,
                company_id: r.company_id,
                created_at: r.created_at,
                updated_at: r.updated_at,
                deleted_at: r.deleted_at,
                synced: r.synced,
            },
            session_id: r.session_id,
            kind: MovementKind::from_str(&r.kind),
            amount: r.amount,
            method: r.method,
            reason: r.reason,
            detail: r.detail,
            order_id: r.order_id,
        }
    }
}

pub struct PgCashMovementRepository {
    pool: PgPool,
}

impl PgCashMovementRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CashMovementRepository for PgCashMovementRepository {
    async fn find_by_id(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<Option<CashMovement>, CoreError> {
        Ok(sqlx::query_as::<_, CashMovementRow>(
            "SELECT * FROM cash_movements WHERE company_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(company_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?
        .map(Into::into))
    }

    async fn find_by_session(
        &self,
        company_id: Uuid,
        session_id: Uuid,
    ) -> Result<Vec<CashMovement>, CoreError> {
        Ok(sqlx::query_as::<_, CashMovementRow>(
            "SELECT * FROM cash_movements
             WHERE company_id = $1 AND session_id = $2 AND deleted_at IS NULL
             ORDER BY created_at ASC",
        )
        .bind(company_id)
        .bind(session_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?
        .into_iter()
        .map(Into::into)
        .collect())
    }

    async fn create(&self, m: &CashMovement) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO cash_movements
             (id, company_id, session_id, kind, amount, method, reason, detail,
              order_id, created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
        )
        .bind(m.base.id)
        .bind(m.base.company_id)
        .bind(m.session_id)
        .bind(m.kind.to_string())
        .bind(m.amount)
        .bind(&m.method)
        .bind(&m.reason)
        .bind(&m.detail)
        .bind(m.order_id)
        .bind(m.base.created_at)
        .bind(m.base.updated_at)
        .bind(m.base.deleted_at)
        .bind(m.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<CashMovement>, CoreError> {
        Ok(sqlx::query_as::<_, CashMovementRow>(
            "SELECT * FROM cash_movements WHERE company_id = $1 AND synced = false",
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
        sqlx::query("UPDATE cash_movements SET synced = true WHERE company_id = $1 AND id = $2 AND updated_at = $3")
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
    ) -> Result<Vec<CashMovement>, CoreError> {
        Ok(sqlx::query_as::<_, CashMovementRow>(
            "SELECT * FROM cash_movements WHERE company_id = $1 AND updated_at > $2",
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

    async fn sync_upsert(&self, m: &CashMovement) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO cash_movements
             (id, company_id, session_id, kind, amount, method, reason, detail,
              order_id, created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
             ON CONFLICT (id) DO UPDATE SET
               kind = EXCLUDED.kind,
               amount = EXCLUDED.amount,
               method = EXCLUDED.method,
               reason = EXCLUDED.reason,
               detail = EXCLUDED.detail,
               order_id = EXCLUDED.order_id,
               updated_at = EXCLUDED.updated_at,
               deleted_at = EXCLUDED.deleted_at,
               synced = EXCLUDED.synced
             WHERE EXCLUDED.updated_at > cash_movements.updated_at AND cash_movements.company_id = EXCLUDED.company_id",
        )
        .bind(m.base.id)
        .bind(m.base.company_id)
        .bind(m.session_id)
        .bind(m.kind.to_string())
        .bind(m.amount)
        .bind(&m.method)
        .bind(&m.reason)
        .bind(&m.detail)
        .bind(m.order_id)
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
