use async_trait::async_trait;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::SqlitePool;
use uuid::Uuid;

use letaf_core::error::CoreError;
use letaf_core::subcategory::model::Subcategory;
use letaf_core::subcategory::repository::SubcategoryRepository;

use super::helpers::{parse_base, map_db, parse_uuid, ts};

#[derive(FromRow)]
struct SubcategoryRow {
    id: String,
    company_id: String,
    category_id: String,
    name: String,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    synced: bool,
    sort_order: i32,
}

impl TryFrom<SubcategoryRow> for Subcategory {
    type Error = CoreError;

    fn try_from(r: SubcategoryRow) -> Result<Self, Self::Error> {
        Ok(Self {
            base: parse_base(&r.id, &r.company_id, &r.created_at, &r.updated_at, r.deleted_at.as_deref(), r.synced)?,
            category_id: parse_uuid(&r.category_id)?,
            name: r.name,
            sort_order: r.sort_order,
        })
    }
}

/// Implementação SQLite do SubcategoryRepository.
///
/// Regras aplicadas (AI_RULES.md §3, §5, §7, §10, §11):
/// - Desktop usa SQLite.
/// - Todas queries filtram por company_id (isolamento).
/// - Soft delete via deleted_at.
pub struct SqliteSubcategoryRepository {
    pool: SqlitePool,
}

impl SqliteSubcategoryRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SubcategoryRepository for SqliteSubcategoryRepository {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Subcategory>, CoreError> {
        let row = sqlx::query_as::<_, SubcategoryRow>(
            "SELECT * FROM subcategories WHERE company_id = ?1 AND id = ?2 AND deleted_at IS NULL",
        )
        .bind(company_id.to_string())
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;

        row.map(Subcategory::try_from).transpose()
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<Subcategory>, CoreError> {
        let rows = sqlx::query_as::<_, SubcategoryRow>(
            "SELECT * FROM subcategories WHERE company_id = ?1 AND deleted_at IS NULL ORDER BY sort_order ASC, name ASC",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        rows.into_iter().map(Subcategory::try_from).collect()
    }

    async fn find_by_category(&self, company_id: Uuid, category_id: Uuid) -> Result<Vec<Subcategory>, CoreError> {
        let rows = sqlx::query_as::<_, SubcategoryRow>(
            "SELECT * FROM subcategories
             WHERE company_id = ?1 AND category_id = ?2 AND deleted_at IS NULL
             ORDER BY sort_order ASC, name ASC",
        )
        .bind(company_id.to_string())
        .bind(category_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        rows.into_iter().map(Subcategory::try_from).collect()
    }

    async fn create(&self, sc: &Subcategory) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO subcategories (id, company_id, category_id, name, created_at, updated_at, deleted_at, synced, sort_order)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )
        .bind(sc.base.id.to_string())
        .bind(sc.base.company_id.to_string())
        .bind(sc.category_id.to_string())
        .bind(&sc.name)
        .bind(ts(sc.base.created_at))
        .bind(ts(sc.base.updated_at))
        .bind(sc.base.deleted_at.map(ts))
        .bind(sc.base.synced)
        .bind(sc.sort_order)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn update(&self, sc: &Subcategory) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE subcategories SET category_id = ?1, name = ?2, updated_at = ?3, synced = ?4, sort_order = ?5
             WHERE company_id = ?6 AND id = ?7 AND deleted_at IS NULL",
        )
        .bind(sc.category_id.to_string())
        .bind(&sc.name)
        .bind(ts(sc.base.updated_at))
        .bind(sc.base.synced)
        .bind(sc.sort_order)
        .bind(sc.base.company_id.to_string())
        .bind(sc.base.id.to_string())
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        sqlx::query(
            "UPDATE subcategories SET deleted_at = ?1, updated_at = ?2, synced = false
             WHERE company_id = ?3 AND id = ?4 AND deleted_at IS NULL",
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

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Subcategory>, CoreError> {
        let rows = sqlx::query_as::<_, SubcategoryRow>(
            "SELECT * FROM subcategories WHERE company_id = ?1 AND synced = false",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        rows.into_iter().map(Subcategory::try_from).collect()
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE subcategories SET synced = true WHERE company_id = ?1 AND id = ?2 AND updated_at = ?3",
        )
        .bind(company_id.to_string())
        .bind(id.to_string())
            .bind(ts(updated_at))
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn find_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<Subcategory>, CoreError> {
        let rows = sqlx::query_as::<_, SubcategoryRow>(
            "SELECT * FROM subcategories WHERE company_id = ?1 AND updated_at > ?2",
        )
        .bind(company_id.to_string())
        .bind(ts(since))
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        rows.into_iter().map(Subcategory::try_from).collect()
    }

    async fn sync_upsert(&self, sc: &Subcategory) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO subcategories (id, company_id, category_id, name, created_at, updated_at, deleted_at, synced, sort_order)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT (id) DO UPDATE SET
                 category_id = excluded.category_id,
                 name = excluded.name,
                 updated_at = excluded.updated_at,
                 deleted_at = excluded.deleted_at,
                 synced = excluded.synced,
                 sort_order = excluded.sort_order
             WHERE excluded.updated_at > subcategories.updated_at",
        )
        .bind(sc.base.id.to_string())
        .bind(sc.base.company_id.to_string())
        .bind(sc.category_id.to_string())
        .bind(&sc.name)
        .bind(ts(sc.base.created_at))
        .bind(ts(sc.base.updated_at))
        .bind(sc.base.deleted_at.map(ts))
        .bind(sc.base.synced)
        .bind(sc.sort_order)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }
}
