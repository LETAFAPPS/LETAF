use async_trait::async_trait;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::PgPool;
use uuid::Uuid;

use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;
use letaf_core::category::model::Category;
use letaf_core::category::repository::CategoryRepository;

use super::helpers::map_db;

#[derive(FromRow)]
struct CategoryRow {
    id: Uuid,
    company_id: Uuid,
    name: String,
    description: Option<String>,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    deleted_at: Option<NaiveDateTime>,
    synced: bool,
    sort_order: i32,
    icon_name: Option<String>,
}

impl From<CategoryRow> for Category {
    fn from(r: CategoryRow) -> Self {
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
            description: r.description,
            sort_order: r.sort_order,
            icon_name: r.icon_name,
        }
    }
}

/// Implementação PostgreSQL do CategoryRepository.
///
/// Regras aplicadas (AI_RULES.md §3, §5, §6, §10):
/// - Todas queries filtram por company_id (isolamento)
/// - Soft delete via deleted_at
/// - Servidor usa PostgreSQL
/// - Acesso ao banco somente via repository
pub struct PgCategoryRepository {
    pool: PgPool,
}

impl PgCategoryRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CategoryRepository for PgCategoryRepository {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Category>, CoreError> {
        sqlx::query_as::<_, CategoryRow>(
            "SELECT * FROM categories WHERE company_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(company_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map(|opt| opt.map(Category::from))
        .map_err(map_db)
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<Category>, CoreError> {
        let rows = sqlx::query_as::<_, CategoryRow>(
            "SELECT * FROM categories WHERE company_id = $1 AND deleted_at IS NULL ORDER BY sort_order ASC, name ASC",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(rows.into_iter().map(Category::from).collect())
    }

    async fn create(&self, category: &Category) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO categories (id, company_id, name, description, created_at, updated_at, deleted_at, synced, sort_order, icon_name)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(category.base.id)
        .bind(category.base.company_id)
        .bind(&category.name)
        .bind(&category.description)
        .bind(category.base.created_at)
        .bind(category.base.updated_at)
        .bind(category.base.deleted_at)
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
            "UPDATE categories SET name = $1, description = $2, updated_at = $3, synced = $4, sort_order = $5, icon_name = $6
             WHERE company_id = $7 AND id = $8 AND deleted_at IS NULL",
        )
        .bind(&category.name)
        .bind(&category.description)
        .bind(category.base.updated_at)
        .bind(category.base.synced)
        .bind(category.sort_order)
        .bind(&category.icon_name)
        .bind(category.base.company_id)
        .bind(category.base.id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        sqlx::query(
            "UPDATE categories SET deleted_at = $1, updated_at = $2, synced = false
             WHERE company_id = $3 AND id = $4 AND deleted_at IS NULL",
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

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Category>, CoreError> {
        let rows = sqlx::query_as::<_, CategoryRow>(
            "SELECT * FROM categories WHERE company_id = $1 AND synced = false",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(rows.into_iter().map(Category::from).collect())
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE categories SET synced = true WHERE company_id = $1 AND id = $2 AND updated_at = $3",
        )
        .bind(company_id)
        .bind(id)
        .bind(updated_at)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    async fn find_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<Category>, CoreError> {
        let rows = sqlx::query_as::<_, CategoryRow>(
            "SELECT * FROM categories WHERE company_id = $1 AND updated_at > $2",
        )
        .bind(company_id)
        .bind(since)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(rows.into_iter().map(Category::from).collect())
    }

    async fn sync_upsert(&self, category: &Category) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO categories (id, company_id, name, description, created_at, updated_at, deleted_at, synced, sort_order, icon_name)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (id) DO UPDATE SET
                 name = EXCLUDED.name,
                 description = EXCLUDED.description,
                 updated_at = EXCLUDED.updated_at,
                 deleted_at = EXCLUDED.deleted_at,
                 synced = EXCLUDED.synced,
                 sort_order = EXCLUDED.sort_order,
                 icon_name = EXCLUDED.icon_name
             WHERE EXCLUDED.updated_at > categories.updated_at AND categories.company_id = EXCLUDED.company_id",
        )
        .bind(category.base.id)
        .bind(category.base.company_id)
        .bind(&category.name)
        .bind(&category.description)
        .bind(category.base.created_at)
        .bind(category.base.updated_at)
        .bind(category.base.deleted_at)
        .bind(category.base.synced)
        .bind(category.sort_order)
        .bind(&category.icon_name)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }
}
