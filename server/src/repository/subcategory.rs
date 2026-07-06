use async_trait::async_trait;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::PgPool;
use uuid::Uuid;

use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;
use letaf_core::subcategory::model::Subcategory;
use letaf_core::subcategory::repository::SubcategoryRepository;

use super::helpers::map_db;

#[derive(FromRow)]
struct SubcategoryRow {
    id: Uuid,
    company_id: Uuid,
    category_id: Uuid,
    name: String,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    deleted_at: Option<NaiveDateTime>,
    synced: bool,
    sort_order: i32,
}

impl From<SubcategoryRow> for Subcategory {
    fn from(r: SubcategoryRow) -> Self {
        Self {
            base: BaseFields {
                id: r.id,
                company_id: r.company_id,
                created_at: r.created_at,
                updated_at: r.updated_at,
                deleted_at: r.deleted_at,
                synced: r.synced,
            },
            category_id: r.category_id,
            name: r.name,
            sort_order: r.sort_order,
        }
    }
}

/// Implementação PostgreSQL do SubcategoryRepository.
///
/// Regras aplicadas (AI_RULES.md §3, §5, §10, §11):
/// - Todas queries filtram por company_id (isolamento).
/// - Soft delete via deleted_at.
/// - Acesso ao banco somente via repository.
pub struct PgSubcategoryRepository {
    pool: PgPool,
}

impl PgSubcategoryRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SubcategoryRepository for PgSubcategoryRepository {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Subcategory>, CoreError> {
        sqlx::query_as::<_, SubcategoryRow>(
            "SELECT * FROM subcategories WHERE company_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(company_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map(|opt| opt.map(Subcategory::from))
        .map_err(map_db)
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<Subcategory>, CoreError> {
        let rows = sqlx::query_as::<_, SubcategoryRow>(
            "SELECT * FROM subcategories WHERE company_id = $1 AND deleted_at IS NULL ORDER BY sort_order ASC, name ASC",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(rows.into_iter().map(Subcategory::from).collect())
    }

    async fn find_by_category(&self, company_id: Uuid, category_id: Uuid) -> Result<Vec<Subcategory>, CoreError> {
        let rows = sqlx::query_as::<_, SubcategoryRow>(
            "SELECT * FROM subcategories
             WHERE company_id = $1 AND category_id = $2 AND deleted_at IS NULL
             ORDER BY sort_order ASC, name ASC",
        )
        .bind(company_id)
        .bind(category_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(rows.into_iter().map(Subcategory::from).collect())
    }

    async fn create(&self, sc: &Subcategory) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO subcategories (id, company_id, category_id, name, created_at, updated_at, deleted_at, synced, sort_order)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(sc.base.id)
        .bind(sc.base.company_id)
        .bind(sc.category_id)
        .bind(&sc.name)
        .bind(sc.base.created_at)
        .bind(sc.base.updated_at)
        .bind(sc.base.deleted_at)
        .bind(sc.base.synced)
        .bind(sc.sort_order)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn update(&self, sc: &Subcategory) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE subcategories SET category_id = $1, name = $2, updated_at = $3, synced = $4, sort_order = $5
             WHERE company_id = $6 AND id = $7 AND deleted_at IS NULL",
        )
        .bind(sc.category_id)
        .bind(&sc.name)
        .bind(sc.base.updated_at)
        .bind(sc.base.synced)
        .bind(sc.sort_order)
        .bind(sc.base.company_id)
        .bind(sc.base.id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        sqlx::query(
            "UPDATE subcategories SET deleted_at = $1, updated_at = $2, synced = false
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

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Subcategory>, CoreError> {
        let rows = sqlx::query_as::<_, SubcategoryRow>(
            "SELECT * FROM subcategories WHERE company_id = $1 AND synced = false",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(rows.into_iter().map(Subcategory::from).collect())
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE subcategories SET synced = true WHERE company_id = $1 AND id = $2 AND updated_at = $3",
        )
        .bind(company_id)
        .bind(id)
        .bind(updated_at)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn find_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<Subcategory>, CoreError> {
        let rows = sqlx::query_as::<_, SubcategoryRow>(
            "SELECT * FROM subcategories WHERE company_id = $1 AND updated_at > $2",
        )
        .bind(company_id)
        .bind(since)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(rows.into_iter().map(Subcategory::from).collect())
    }

    async fn sync_upsert(&self, sc: &Subcategory) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO subcategories (id, company_id, category_id, name, created_at, updated_at, deleted_at, synced, sort_order)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (id) DO UPDATE SET
                 category_id = EXCLUDED.category_id,
                 name = EXCLUDED.name,
                 updated_at = EXCLUDED.updated_at,
                 deleted_at = EXCLUDED.deleted_at,
                 synced = EXCLUDED.synced,
                 sort_order = EXCLUDED.sort_order
             WHERE EXCLUDED.updated_at > subcategories.updated_at AND subcategories.company_id = EXCLUDED.company_id",
        )
        .bind(sc.base.id)
        .bind(sc.base.company_id)
        .bind(sc.category_id)
        .bind(&sc.name)
        .bind(sc.base.created_at)
        .bind(sc.base.updated_at)
        .bind(sc.base.deleted_at)
        .bind(sc.base.synced)
        .bind(sc.sort_order)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }
}
