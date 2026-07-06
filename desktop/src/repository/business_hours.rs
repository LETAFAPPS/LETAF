use async_trait::async_trait;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::SqlitePool;
use uuid::Uuid;

use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;
use letaf_core::business_hours::model::BusinessHours;
use letaf_core::business_hours::repository::BusinessHoursRepository;

use super::helpers::{map_db, parse_timestamp, parse_uuid, ts};

#[derive(FromRow)]
struct BusinessHoursRow {
    id: String,
    company_id: String,
    day_of_week: i32,
    open_time: String,
    close_time: String,
    is_open: bool,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    synced: bool,
}

impl TryFrom<BusinessHoursRow> for BusinessHours {
    type Error = CoreError;

    fn try_from(r: BusinessHoursRow) -> Result<Self, Self::Error> {
        Ok(Self {
            base: BaseFields {
                id: parse_uuid(&r.id)?,
                company_id: parse_uuid(&r.company_id)?,
                created_at: parse_timestamp(&r.created_at)?,
                updated_at: parse_timestamp(&r.updated_at)?,
                deleted_at: r.deleted_at.as_deref().map(parse_timestamp).transpose()?,
                synced: r.synced,
            },
            day_of_week: r.day_of_week,
            open_time: r.open_time,
            close_time: r.close_time,
            is_open: r.is_open,
        })
    }
}

/// Implementação SQLite do BusinessHoursRepository.
///
/// Regras aplicadas (AI_RULES.md §3, §5, §7, §10):
/// - Desktop usa SQLite
/// - Todas queries filtram por company_id
/// - Upsert via ON CONFLICT (company_id, day_of_week)
pub struct SqliteBusinessHoursRepository {
    pool: SqlitePool,
}

impl SqliteBusinessHoursRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl BusinessHoursRepository for SqliteBusinessHoursRepository {
    async fn find_all(&self, company_id: Uuid) -> Result<Vec<BusinessHours>, CoreError> {
        let rows = sqlx::query_as::<_, BusinessHoursRow>(
            "SELECT * FROM business_hours WHERE company_id = ?1 AND deleted_at IS NULL ORDER BY day_of_week ASC",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        rows.into_iter().map(BusinessHours::try_from).collect()
    }

    async fn find_by_day(
        &self,
        company_id: Uuid,
        day_of_week: i32,
    ) -> Result<Option<BusinessHours>, CoreError> {
        let row = sqlx::query_as::<_, BusinessHoursRow>(
            "SELECT * FROM business_hours WHERE company_id = ?1 AND day_of_week = ?2 AND deleted_at IS NULL",
        )
        .bind(company_id.to_string())
        .bind(day_of_week)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;

        row.map(BusinessHours::try_from).transpose()
    }

    async fn upsert(&self, hours: &BusinessHours) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO business_hours
                (id, company_id, day_of_week, open_time, close_time, is_open,
                 created_at, updated_at, deleted_at, synced)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(company_id, day_of_week) DO UPDATE SET
                 open_time  = excluded.open_time,
                 close_time = excluded.close_time,
                 is_open    = excluded.is_open,
                 updated_at = excluded.updated_at,
                 synced     = excluded.synced",
        )
        .bind(hours.base.id.to_string())
        .bind(hours.base.company_id.to_string())
        .bind(hours.day_of_week)
        .bind(&hours.open_time)
        .bind(&hours.close_time)
        .bind(hours.is_open)
        .bind(ts(hours.base.created_at))
        .bind(ts(hours.base.updated_at))
        .bind(hours.base.deleted_at.map(ts))
        .bind(hours.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<BusinessHours>, CoreError> {
        let rows = sqlx::query_as::<_, BusinessHoursRow>(
            "SELECT * FROM business_hours WHERE company_id = ?1 AND synced = false",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        rows.into_iter().map(BusinessHours::try_from).collect()
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE business_hours SET synced = true WHERE company_id = ?1 AND id = ?2 AND updated_at = ?3",
        )
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
    ) -> Result<Vec<BusinessHours>, CoreError> {
        let rows = sqlx::query_as::<_, BusinessHoursRow>(
            "SELECT * FROM business_hours WHERE company_id = ?1 AND updated_at > ?2",
        )
        .bind(company_id.to_string())
        .bind(ts(since))
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        rows.into_iter().map(BusinessHours::try_from).collect()
    }

    async fn sync_upsert(&self, hours: &BusinessHours) -> Result<(), CoreError> {
        // Resolve conflito pela chave NATURAL (company_id, day_of_week),
        // não por `id`. A tabela tem UNIQUE(company_id, day_of_week);
        // se o servidor enviar um registro do mesmo dia com `id`
        // diferente (criado em outro dispositivo/web), o antigo
        // `ON CONFLICT(id)` não detectava o conflito e o INSERT
        // falhava na constraint única — o item era perdido no ciclo
        // e re-tentado infinitamente. `id` de business_hours não é
        // alvo de FK, então manter o id local é seguro. LWW preservado.
        sqlx::query(
            "INSERT INTO business_hours
                (id, company_id, day_of_week, open_time, close_time, is_open,
                 created_at, updated_at, deleted_at, synced)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(company_id, day_of_week) DO UPDATE SET
                 open_time  = excluded.open_time,
                 close_time = excluded.close_time,
                 is_open    = excluded.is_open,
                 updated_at = excluded.updated_at,
                 deleted_at = excluded.deleted_at,
                 synced     = excluded.synced
             WHERE excluded.updated_at > business_hours.updated_at",
        )
        .bind(hours.base.id.to_string())
        .bind(hours.base.company_id.to_string())
        .bind(hours.day_of_week)
        .bind(&hours.open_time)
        .bind(&hours.close_time)
        .bind(hours.is_open)
        .bind(ts(hours.base.created_at))
        .bind(ts(hours.base.updated_at))
        .bind(hours.base.deleted_at.map(ts))
        .bind(hours.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }
}
