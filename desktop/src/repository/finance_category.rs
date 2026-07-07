use async_trait::async_trait;
use chrono::{NaiveDateTime, Utc};
use sqlx::prelude::FromRow;
use sqlx::SqlitePool;
use uuid::Uuid;

use letaf_core::error::CoreError;
use letaf_core::finance_category::model::{FinanceCategory, FinanceCategoryScope};
use letaf_core::finance_category::repository::FinanceCategoryRepository;

use super::helpers::{parse_base, map_db, ts};

#[derive(FromRow)]
struct FinanceCategoryRow {
    id: String,
    company_id: String,
    name: String,
    color: String,
    icon: String,
    scope: String,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    synced: bool,
}

impl TryFrom<FinanceCategoryRow> for FinanceCategory {
    type Error = CoreError;
    fn try_from(r: FinanceCategoryRow) -> Result<Self, Self::Error> {
        Ok(Self {
            base: parse_base(&r.id, &r.company_id, &r.created_at, &r.updated_at, r.deleted_at.as_deref(), r.synced)?,
            name: r.name,
            color: r.color,
            icon: r.icon,
            scope: FinanceCategoryScope::from_str(&r.scope),
        })
    }
}

pub struct SqliteFinanceCategoryRepository {
    pool: SqlitePool,
}

impl SqliteFinanceCategoryRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl FinanceCategoryRepository for SqliteFinanceCategoryRepository {
    async fn find_by_id(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<Option<FinanceCategory>, CoreError> {
        let row = sqlx::query_as::<_, FinanceCategoryRow>(
            "SELECT * FROM finance_categories
             WHERE company_id = ? AND id = ? AND deleted_at IS NULL",
        )
        .bind(company_id.to_string())
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        row.map(FinanceCategory::try_from).transpose()
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<FinanceCategory>, CoreError> {
        let rows = sqlx::query_as::<_, FinanceCategoryRow>(
            "SELECT * FROM finance_categories
             WHERE company_id = ? AND deleted_at IS NULL
             ORDER BY name",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(FinanceCategory::try_from).collect()
    }

    async fn create(&self, c: &FinanceCategory) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO finance_categories
             (id, company_id, name, color, icon, scope,
              created_at, updated_at, deleted_at, synced)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(c.base.id.to_string())
        .bind(c.base.company_id.to_string())
        .bind(&c.name)
        .bind(&c.color)
        .bind(&c.icon)
        .bind(c.scope.to_string())
        .bind(ts(c.base.created_at))
        .bind(ts(c.base.updated_at))
        .bind(c.base.deleted_at.map(ts))
        .bind(c.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn update(&self, c: &FinanceCategory) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE finance_categories SET
               name = ?, color = ?, icon = ?, scope = ?,
               updated_at = ?, deleted_at = ?, synced = ?
             WHERE company_id = ? AND id = ?",
        )
        .bind(&c.name)
        .bind(&c.color)
        .bind(&c.icon)
        .bind(c.scope.to_string())
        .bind(ts(c.base.updated_at))
        .bind(c.base.deleted_at.map(ts))
        .bind(c.base.synced)
        .bind(c.base.company_id.to_string())
        .bind(c.base.id.to_string())
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let now = Utc::now().naive_utc();
        sqlx::query(
            "UPDATE finance_categories
               SET deleted_at = ?, updated_at = ?, synced = 0
             WHERE company_id = ? AND id = ?",
        )
        .bind(ts(now))
        .bind(ts(now))
        .bind(company_id.to_string())
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    // ── Sync ──

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<FinanceCategory>, CoreError> {
        let rows = sqlx::query_as::<_, FinanceCategoryRow>(
            "SELECT * FROM finance_categories WHERE company_id = ? AND synced = 0",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(FinanceCategory::try_from).collect()
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE finance_categories SET synced = 1 WHERE company_id = ? AND id = ? AND updated_at = ?",
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
    ) -> Result<Vec<FinanceCategory>, CoreError> {
        let rows = sqlx::query_as::<_, FinanceCategoryRow>(
            "SELECT * FROM finance_categories
             WHERE company_id = ? AND updated_at > ?",
        )
        .bind(company_id.to_string())
        .bind(ts(since))
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(FinanceCategory::try_from).collect()
    }

    async fn sync_upsert(&self, c: &FinanceCategory) -> Result<(), CoreError> {
        // UPSERT com guard de updated_at (last-write-wins, AI_RULES §7.7).
        sqlx::query(
            "INSERT INTO finance_categories
             (id, company_id, name, color, icon, scope,
              created_at, updated_at, deleted_at, synced)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
               name = excluded.name,
               color = excluded.color,
               icon = excluded.icon,
               scope = excluded.scope,
               updated_at = excluded.updated_at,
               deleted_at = excluded.deleted_at,
               synced = excluded.synced
             WHERE excluded.updated_at > finance_categories.updated_at",
        )
        .bind(c.base.id.to_string())
        .bind(c.base.company_id.to_string())
        .bind(&c.name)
        .bind(&c.color)
        .bind(&c.icon)
        .bind(c.scope.to_string())
        .bind(ts(c.base.created_at))
        .bind(ts(c.base.updated_at))
        .bind(c.base.deleted_at.map(ts))
        .bind(c.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }
}
