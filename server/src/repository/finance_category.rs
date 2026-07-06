use async_trait::async_trait;
use chrono::{NaiveDateTime, Utc};
use sqlx::prelude::FromRow;
use sqlx::PgPool;
use uuid::Uuid;

use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;
use letaf_core::finance_category::model::{FinanceCategory, FinanceCategoryScope};
use letaf_core::finance_category::repository::FinanceCategoryRepository;

use super::helpers::map_db;

#[derive(FromRow)]
struct FinanceCategoryRow {
    id: Uuid,
    company_id: Uuid,
    name: String,
    color: String,
    icon: String,
    scope: String,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    deleted_at: Option<NaiveDateTime>,
    synced: bool,
}

impl From<FinanceCategoryRow> for FinanceCategory {
    fn from(r: FinanceCategoryRow) -> Self {
        Self {
            base: BaseFields {
                id: r.id,
                company_id: r.company_id,
                created_at: r.created_at,
                updated_at: r.updated_at,
                deleted_at: r.deleted_at,
                synced: r.synced,
            },
            name: r.name,
            color: r.color,
            icon: r.icon,
            scope: FinanceCategoryScope::from_str(&r.scope),
        }
    }
}

pub struct PgFinanceCategoryRepository {
    pool: PgPool,
}

impl PgFinanceCategoryRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl FinanceCategoryRepository for PgFinanceCategoryRepository {
    async fn find_by_id(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<Option<FinanceCategory>, CoreError> {
        Ok(sqlx::query_as::<_, FinanceCategoryRow>(
            "SELECT * FROM finance_categories
             WHERE company_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(company_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?
        .map(Into::into))
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<FinanceCategory>, CoreError> {
        Ok(sqlx::query_as::<_, FinanceCategoryRow>(
            "SELECT * FROM finance_categories
             WHERE company_id = $1 AND deleted_at IS NULL
             ORDER BY name",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?
        .into_iter()
        .map(Into::into)
        .collect())
    }

    async fn create(&self, c: &FinanceCategory) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO finance_categories
             (id, company_id, name, color, icon, scope,
              created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(c.base.id)
        .bind(c.base.company_id)
        .bind(&c.name)
        .bind(&c.color)
        .bind(&c.icon)
        .bind(c.scope.to_string())
        .bind(c.base.created_at)
        .bind(c.base.updated_at)
        .bind(c.base.deleted_at)
        .bind(c.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn update(&self, c: &FinanceCategory) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE finance_categories SET
               name = $1, color = $2, icon = $3, scope = $4,
               updated_at = $5, deleted_at = $6, synced = $7
             WHERE company_id = $8 AND id = $9",
        )
        .bind(&c.name)
        .bind(&c.color)
        .bind(&c.icon)
        .bind(c.scope.to_string())
        .bind(c.base.updated_at)
        .bind(c.base.deleted_at)
        .bind(c.base.synced)
        .bind(c.base.company_id)
        .bind(c.base.id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let now = Utc::now().naive_utc();
        sqlx::query(
            "UPDATE finance_categories
               SET deleted_at = $1, updated_at = $2, synced = FALSE
             WHERE company_id = $3 AND id = $4",
        )
        .bind(now)
        .bind(now)
        .bind(company_id)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    // ── Sync ──

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<FinanceCategory>, CoreError> {
        Ok(sqlx::query_as::<_, FinanceCategoryRow>(
            "SELECT * FROM finance_categories WHERE company_id = $1 AND synced = FALSE",
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
        sqlx::query(
            "UPDATE finance_categories SET synced = TRUE
             WHERE company_id = $1 AND id = $2 AND updated_at = $3",
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
    ) -> Result<Vec<FinanceCategory>, CoreError> {
        Ok(sqlx::query_as::<_, FinanceCategoryRow>(
            "SELECT * FROM finance_categories
             WHERE company_id = $1 AND updated_at > $2",
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

    async fn sync_upsert(&self, c: &FinanceCategory) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO finance_categories
             (id, company_id, name, color, icon, scope,
              created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (id) DO UPDATE SET
               name = EXCLUDED.name,
               color = EXCLUDED.color,
               icon = EXCLUDED.icon,
               scope = EXCLUDED.scope,
               updated_at = EXCLUDED.updated_at,
               deleted_at = EXCLUDED.deleted_at,
               synced = EXCLUDED.synced
             WHERE EXCLUDED.updated_at > finance_categories.updated_at AND finance_categories.company_id = EXCLUDED.company_id",
        )
        .bind(c.base.id)
        .bind(c.base.company_id)
        .bind(&c.name)
        .bind(&c.color)
        .bind(&c.icon)
        .bind(c.scope.to_string())
        .bind(c.base.created_at)
        .bind(c.base.updated_at)
        .bind(c.base.deleted_at)
        .bind(c.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }
}
