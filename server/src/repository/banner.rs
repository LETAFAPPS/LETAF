use async_trait::async_trait;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::PgPool;
use uuid::Uuid;

use letaf_core::banner::model::Banner;
use letaf_core::banner::repository::BannerRepository;
use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;

use super::helpers::map_db;

#[derive(FromRow)]
struct BannerRow {
    id: Uuid,
    company_id: Uuid,
    title: String,
    image_data: String,
    item_type: String,
    item_id: Option<Uuid>,
    item_url: Option<String>,
    active: bool,
    sort_order: i32,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    deleted_at: Option<NaiveDateTime>,
    synced: bool,
}

impl From<BannerRow> for Banner {
    fn from(r: BannerRow) -> Self {
        Self {
            base: BaseFields {
                id: r.id,
                company_id: r.company_id,
                created_at: r.created_at,
                updated_at: r.updated_at,
                deleted_at: r.deleted_at,
                synced: r.synced,
            },
            title: r.title,
            image_data: r.image_data,
            item_type: r.item_type,
            item_id: r.item_id,
            item_url: r.item_url,
            active: r.active,
            sort_order: r.sort_order,
        }
    }
}

pub struct PgBannerRepository {
    pool: PgPool,
}

impl PgBannerRepository {
    pub fn new(pool: PgPool) -> Self { Self { pool } }
}

#[async_trait]
impl BannerRepository for PgBannerRepository {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Banner>, CoreError> {
        sqlx::query_as::<_, BannerRow>(
            "SELECT * FROM banners WHERE company_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(company_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map(|opt| opt.map(Banner::from))
        .map_err(map_db)
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<Banner>, CoreError> {
        let rows = sqlx::query_as::<_, BannerRow>(
            "SELECT * FROM banners WHERE company_id = $1 AND deleted_at IS NULL ORDER BY sort_order ASC, created_at DESC",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(Banner::from).collect())
    }

    async fn find_active(&self, company_id: Uuid) -> Result<Vec<Banner>, CoreError> {
        let rows = sqlx::query_as::<_, BannerRow>(
            "SELECT * FROM banners WHERE company_id = $1 AND deleted_at IS NULL AND active = TRUE ORDER BY sort_order ASC, created_at DESC",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(Banner::from).collect())
    }

    async fn create(&self, banner: &Banner) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO banners (id, company_id, title, image_data, item_type, item_id, item_url, active, sort_order, created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
        )
        .bind(banner.base.id)
        .bind(banner.base.company_id)
        .bind(&banner.title)
        .bind(&banner.image_data)
        .bind(&banner.item_type)
        .bind(banner.item_id)
        .bind(&banner.item_url)
        .bind(banner.active)
        .bind(banner.sort_order)
        .bind(banner.base.created_at)
        .bind(banner.base.updated_at)
        .bind(banner.base.deleted_at)
        .bind(banner.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn update(&self, banner: &Banner) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE banners SET title = $1, image_data = $2, item_type = $3, item_id = $4, item_url = $5, active = $6, sort_order = $7, updated_at = $8, synced = $9
             WHERE company_id = $10 AND id = $11 AND deleted_at IS NULL",
        )
        .bind(&banner.title)
        .bind(&banner.image_data)
        .bind(&banner.item_type)
        .bind(banner.item_id)
        .bind(&banner.item_url)
        .bind(banner.active)
        .bind(banner.sort_order)
        .bind(banner.base.updated_at)
        .bind(banner.base.synced)
        .bind(banner.base.company_id)
        .bind(banner.base.id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        sqlx::query(
            "UPDATE banners SET deleted_at = $1, updated_at = $2, synced = false WHERE company_id = $3 AND id = $4 AND deleted_at IS NULL",
        )
        .bind(now).bind(now).bind(company_id).bind(id)
        .execute(&self.pool).await.map_err(map_db)?;
        Ok(())
    }

    async fn set_active(&self, company_id: Uuid, id: Uuid, active: bool) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        sqlx::query(
            "UPDATE banners SET active = $1, updated_at = $2, synced = false WHERE company_id = $3 AND id = $4 AND deleted_at IS NULL",
        )
        .bind(active).bind(now).bind(company_id).bind(id)
        .execute(&self.pool).await.map_err(map_db)?;
        Ok(())
    }

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Banner>, CoreError> {
        let rows = sqlx::query_as::<_, BannerRow>(
            "SELECT * FROM banners WHERE company_id = $1 AND synced = false",
        )
        .bind(company_id).fetch_all(&self.pool).await.map_err(map_db)?;
        Ok(rows.into_iter().map(Banner::from).collect())
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query("UPDATE banners SET synced = true WHERE company_id = $1 AND id = $2 AND updated_at = $3")
            .bind(company_id).bind(id)
        .bind(updated_at)
            .execute(&self.pool).await.map_err(map_db)?;
        Ok(())
    }

    async fn find_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<Banner>, CoreError> {
        let rows = sqlx::query_as::<_, BannerRow>(
            "SELECT * FROM banners WHERE company_id = $1 AND updated_at > $2",
        )
        .bind(company_id).bind(since).fetch_all(&self.pool).await.map_err(map_db)?;
        Ok(rows.into_iter().map(Banner::from).collect())
    }

    async fn sync_upsert(&self, banner: &Banner) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO banners (id, company_id, title, image_data, item_type, item_id, item_url, active, sort_order, created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
             ON CONFLICT (id) DO UPDATE SET
                 title = EXCLUDED.title,
                 image_data = EXCLUDED.image_data,
                 item_type = EXCLUDED.item_type,
                 item_id = EXCLUDED.item_id,
                 item_url = EXCLUDED.item_url,
                 active = EXCLUDED.active,
                 sort_order = EXCLUDED.sort_order,
                 updated_at = EXCLUDED.updated_at,
                 deleted_at = EXCLUDED.deleted_at,
                 synced = EXCLUDED.synced
             WHERE EXCLUDED.updated_at > banners.updated_at AND banners.company_id = EXCLUDED.company_id",
        )
        .bind(banner.base.id)
        .bind(banner.base.company_id)
        .bind(&banner.title)
        .bind(&banner.image_data)
        .bind(&banner.item_type)
        .bind(banner.item_id)
        .bind(&banner.item_url)
        .bind(banner.active)
        .bind(banner.sort_order)
        .bind(banner.base.created_at)
        .bind(banner.base.updated_at)
        .bind(banner.base.deleted_at)
        .bind(banner.base.synced)
        .execute(&self.pool).await.map_err(map_db)?;
        Ok(())
    }
}
