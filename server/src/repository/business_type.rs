use async_trait::async_trait;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::PgPool;
use uuid::Uuid;

use letaf_core::business_type::model::BusinessType;
use letaf_core::business_type::repository::BusinessTypeRepository;
use letaf_core::error::CoreError;

use super::helpers::map_db;

#[derive(FromRow)]
struct BusinessTypeRow {
    id: Uuid,
    name: String,
    description: String,
    theme: String,
    active: bool,
    sort_order: i32,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    deleted_at: Option<NaiveDateTime>,
}

impl From<BusinessTypeRow> for BusinessType {
    fn from(r: BusinessTypeRow) -> Self {
        Self {
            id: r.id,
            name: r.name,
            description: r.description,
            theme: r.theme,
            active: r.active,
            sort_order: r.sort_order,
            created_at: r.created_at,
            updated_at: r.updated_at,
            deleted_at: r.deleted_at,
        }
    }
}

pub struct PgBusinessTypeRepository {
    pool: PgPool,
}

impl PgBusinessTypeRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl BusinessTypeRepository for PgBusinessTypeRepository {
    async fn find_all(&self) -> Result<Vec<BusinessType>, CoreError> {
        let rows = sqlx::query_as::<_, BusinessTypeRow>(
            "SELECT * FROM business_types WHERE deleted_at IS NULL ORDER BY sort_order, created_at",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(BusinessType::from).collect())
    }

    async fn find_active(&self) -> Result<Vec<BusinessType>, CoreError> {
        let rows = sqlx::query_as::<_, BusinessTypeRow>(
            "SELECT * FROM business_types WHERE deleted_at IS NULL AND active = TRUE ORDER BY sort_order, created_at",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(BusinessType::from).collect())
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<BusinessType>, CoreError> {
        let row = sqlx::query_as::<_, BusinessTypeRow>(
            "SELECT * FROM business_types WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(row.map(BusinessType::from))
    }

    async fn create(&self, item: &BusinessType) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO business_types (id, name, description, theme, active, sort_order, created_at, updated_at, deleted_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
        )
        .bind(item.id)
        .bind(&item.name)
        .bind(&item.description)
        .bind(&item.theme)
        .bind(item.active)
        .bind(item.sort_order)
        .bind(item.created_at)
        .bind(item.updated_at)
        .bind(item.deleted_at)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn update(&self, item: &BusinessType) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE business_types SET name = $1, description = $2, theme = $3, active = $4, sort_order = $5, updated_at = $6
             WHERE id = $7",
        )
        .bind(&item.name)
        .bind(&item.description)
        .bind(&item.theme)
        .bind(item.active)
        .bind(item.sort_order)
        .bind(item.updated_at)
        .bind(item.id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn soft_delete(&self, id: Uuid) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        sqlx::query("UPDATE business_types SET deleted_at = $1, updated_at = $1 WHERE id = $2 AND deleted_at IS NULL")
            .bind(now)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(map_db)?;
        Ok(())
    }
}
