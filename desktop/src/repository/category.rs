use async_trait::async_trait;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::SqlitePool;
use uuid::Uuid;

use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;
use letaf_core::category::model::Category;
use letaf_core::category::repository::CategoryRepository;

use super::helpers::{map_db, parse_timestamp, parse_uuid, ts};

#[derive(FromRow)]
struct CategoryRow {
    id: String,
    company_id: String,
    name: String,
    description: Option<String>,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    synced: bool,
    sort_order: i32,
    icon_name: Option<String>,
}

impl TryFrom<CategoryRow> for Category {
    type Error = CoreError;

    fn try_from(r: CategoryRow) -> Result<Self, Self::Error> {
        Ok(Self {
            base: BaseFields {
                id: parse_uuid(&r.id)?,
                company_id: parse_uuid(&r.company_id)?,
                created_at: parse_timestamp(&r.created_at)?,
                updated_at: parse_timestamp(&r.updated_at)?,
                deleted_at: r.deleted_at.as_deref().map(parse_timestamp).transpose()?,
                synced: r.synced,
            },
            name: r.name,
            description: r.description,
            sort_order: r.sort_order,
            icon_name: r.icon_name,
        })
    }
}

/// Implementação SQLite do CategoryRepository.
///
/// Regras aplicadas (AI_RULES.md §3, §5, §7, §10):
/// - Desktop usa SQLite
/// - Todas queries filtram por company_id (isolamento)
/// - Soft delete via deleted_at
pub struct SqliteCategoryRepository {
    pool: SqlitePool,
}

impl SqliteCategoryRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CategoryRepository for SqliteCategoryRepository {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Category>, CoreError> {
        let row = sqlx::query_as::<_, CategoryRow>(
            "SELECT * FROM categories WHERE company_id = ?1 AND id = ?2 AND deleted_at IS NULL",
        )
        .bind(company_id.to_string())
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;

        row.map(Category::try_from).transpose()
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<Category>, CoreError> {
        let rows = sqlx::query_as::<_, CategoryRow>(
            "SELECT * FROM categories WHERE company_id = ?1 AND deleted_at IS NULL ORDER BY sort_order ASC, name ASC",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        rows.into_iter().map(Category::try_from).collect()
    }

    async fn create(&self, category: &Category) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO categories (id, company_id, name, description, created_at, updated_at, deleted_at, synced, sort_order, icon_name)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )
        .bind(category.base.id.to_string())
        .bind(category.base.company_id.to_string())
        .bind(&category.name)
        .bind(&category.description)
        .bind(ts(category.base.created_at))
        .bind(ts(category.base.updated_at))
        .bind(category.base.deleted_at.map(ts))
        .bind(category.base.synced)
        .bind(category.sort_order)
        .bind(&category.icon_name)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    async fn update(&self, category: &Category) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE categories SET name = ?1, description = ?2, updated_at = ?3, synced = ?4, sort_order = ?5, icon_name = ?6
             WHERE company_id = ?7 AND id = ?8 AND deleted_at IS NULL",
        )
        .bind(&category.name)
        .bind(&category.description)
        .bind(ts(category.base.updated_at))
        .bind(category.base.synced)
        .bind(category.sort_order)
        .bind(&category.icon_name)
        .bind(category.base.company_id.to_string())
        .bind(category.base.id.to_string())
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        sqlx::query(
            "UPDATE categories SET deleted_at = ?1, updated_at = ?2, synced = false
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

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Category>, CoreError> {
        let rows = sqlx::query_as::<_, CategoryRow>(
            "SELECT * FROM categories WHERE company_id = ?1 AND synced = false",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        rows.into_iter().map(Category::try_from).collect()
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE categories SET synced = true WHERE company_id = ?1 AND id = ?2 AND updated_at = ?3",
        )
        .bind(company_id.to_string())
        .bind(id.to_string())
            .bind(ts(updated_at))
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    async fn find_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<Category>, CoreError> {
        let rows = sqlx::query_as::<_, CategoryRow>(
            "SELECT * FROM categories WHERE company_id = ?1 AND updated_at > ?2",
        )
        .bind(company_id.to_string())
        .bind(ts(since))
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        rows.into_iter().map(Category::try_from).collect()
    }

    async fn sync_upsert(&self, category: &Category) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO categories (id, company_id, name, description, created_at, updated_at, deleted_at, synced, sort_order, icon_name)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT (id) DO UPDATE SET
                 name = excluded.name,
                 description = excluded.description,
                 updated_at = excluded.updated_at,
                 deleted_at = excluded.deleted_at,
                 synced = excluded.synced,
                 sort_order = excluded.sort_order,
                 icon_name = excluded.icon_name
             WHERE excluded.updated_at > categories.updated_at",
        )
        .bind(category.base.id.to_string())
        .bind(category.base.company_id.to_string())
        .bind(&category.name)
        .bind(&category.description)
        .bind(ts(category.base.created_at))
        .bind(ts(category.base.updated_at))
        .bind(category.base.deleted_at.map(ts))
        .bind(category.base.synced)
        .bind(category.sort_order)
        .bind(&category.icon_name)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }
}
