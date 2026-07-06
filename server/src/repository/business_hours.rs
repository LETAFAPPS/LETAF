use async_trait::async_trait;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::PgPool;
use uuid::Uuid;

use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;
use letaf_core::business_hours::model::BusinessHours;
use letaf_core::business_hours::repository::BusinessHoursRepository;

use super::helpers::map_db;

#[derive(FromRow)]
struct BusinessHoursRow {
    id: Uuid,
    company_id: Uuid,
    day_of_week: i32,
    open_time: String,
    close_time: String,
    is_open: bool,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    deleted_at: Option<NaiveDateTime>,
    synced: bool,
}

impl From<BusinessHoursRow> for BusinessHours {
    fn from(r: BusinessHoursRow) -> Self {
        Self {
            base: BaseFields {
                id: r.id,
                company_id: r.company_id,
                created_at: r.created_at,
                updated_at: r.updated_at,
                deleted_at: r.deleted_at,
                synced: r.synced,
            },
            day_of_week: r.day_of_week,
            open_time: r.open_time,
            close_time: r.close_time,
            is_open: r.is_open,
        }
    }
}

/// Implementação PostgreSQL do BusinessHoursRepository.
///
/// Regras aplicadas (AI_RULES.md §3, §5, §6, §10):
/// - Todas queries filtram por company_id (isolamento multi-tenant)
/// - Upsert via ON CONFLICT (company_id, day_of_week)
/// - Servidor usa PostgreSQL
pub struct PgBusinessHoursRepository {
    pool: PgPool,
}

impl PgBusinessHoursRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl BusinessHoursRepository for PgBusinessHoursRepository {
    async fn find_all(&self, company_id: Uuid) -> Result<Vec<BusinessHours>, CoreError> {
        let rows = sqlx::query_as::<_, BusinessHoursRow>(
            "SELECT * FROM business_hours WHERE company_id = $1 AND deleted_at IS NULL ORDER BY day_of_week ASC",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(rows.into_iter().map(BusinessHours::from).collect())
    }

    async fn find_by_day(
        &self,
        company_id: Uuid,
        day_of_week: i32,
    ) -> Result<Option<BusinessHours>, CoreError> {
        let row = sqlx::query_as::<_, BusinessHoursRow>(
            "SELECT * FROM business_hours WHERE company_id = $1 AND day_of_week = $2 AND deleted_at IS NULL",
        )
        .bind(company_id)
        .bind(day_of_week)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(row.map(BusinessHours::from))
    }

    async fn upsert(&self, hours: &BusinessHours) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO business_hours
                (id, company_id, day_of_week, open_time, close_time, is_open,
                 created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT(company_id, day_of_week) DO UPDATE SET
                 open_time  = EXCLUDED.open_time,
                 close_time = EXCLUDED.close_time,
                 is_open    = EXCLUDED.is_open,
                 updated_at = EXCLUDED.updated_at,
                 synced     = EXCLUDED.synced",
        )
        .bind(hours.base.id)
        .bind(hours.base.company_id)
        .bind(hours.day_of_week)
        .bind(&hours.open_time)
        .bind(&hours.close_time)
        .bind(hours.is_open)
        .bind(hours.base.created_at)
        .bind(hours.base.updated_at)
        .bind(hours.base.deleted_at)
        .bind(hours.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<BusinessHours>, CoreError> {
        let rows = sqlx::query_as::<_, BusinessHoursRow>(
            "SELECT * FROM business_hours WHERE company_id = $1 AND synced = FALSE",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(rows.into_iter().map(BusinessHours::from).collect())
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE business_hours SET synced = TRUE WHERE company_id = $1 AND id = $2",
        )
        .bind(company_id)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
    ) -> Result<Vec<BusinessHours>, CoreError> {
        let rows = sqlx::query_as::<_, BusinessHoursRow>(
            "SELECT * FROM business_hours WHERE company_id = $1 AND updated_at > $2",
        )
        .bind(company_id)
        .bind(since)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(rows.into_iter().map(BusinessHours::from).collect())
    }

    async fn sync_upsert(&self, hours: &BusinessHours) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO business_hours
                (id, company_id, day_of_week, open_time, close_time, is_open,
                 created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT(id) DO UPDATE SET
                 open_time  = EXCLUDED.open_time,
                 close_time = EXCLUDED.close_time,
                 is_open    = EXCLUDED.is_open,
                 updated_at = EXCLUDED.updated_at,
                 deleted_at = EXCLUDED.deleted_at,
                 synced     = EXCLUDED.synced
             WHERE EXCLUDED.updated_at > business_hours.updated_at AND business_hours.company_id = EXCLUDED.company_id",
        )
        .bind(hours.base.id)
        .bind(hours.base.company_id)
        .bind(hours.day_of_week)
        .bind(&hours.open_time)
        .bind(&hours.close_time)
        .bind(hours.is_open)
        .bind(hours.base.created_at)
        .bind(hours.base.updated_at)
        .bind(hours.base.deleted_at)
        .bind(hours.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }
}
