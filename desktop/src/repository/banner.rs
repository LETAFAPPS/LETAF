use async_trait::async_trait;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::SqlitePool;
use uuid::Uuid;

use letaf_core::banner::model::Banner;
use letaf_core::banner::repository::BannerRepository;
use letaf_core::error::CoreError;

use super::helpers::{parse_base, map_db, parse_uuid, ts};

#[derive(FromRow)]
struct BannerRow {
    id: String,
    company_id: String,
    title: String,
    image_data: String,
    item_type: String,
    item_id: Option<String>,
    item_url: Option<String>,
    active: bool,
    sort_order: i32,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    synced: bool,
}

impl TryFrom<BannerRow> for Banner {
    type Error = CoreError;
    fn try_from(r: BannerRow) -> Result<Self, Self::Error> {
        Ok(Self {
            base: parse_base(&r.id, &r.company_id, &r.created_at, &r.updated_at, r.deleted_at.as_deref(), r.synced)?,
            title: r.title,
            image_data: r.image_data,
            item_type: r.item_type,
            item_id: r.item_id.as_deref().map(parse_uuid).transpose()?,
            item_url: r.item_url,
            active: r.active,
            sort_order: r.sort_order,
        })
    }
}

pub struct SqliteBannerRepository {
    pool: SqlitePool,
}

impl SqliteBannerRepository {
    pub fn new(pool: SqlitePool) -> Self { Self { pool } }
}

#[async_trait]
impl BannerRepository for SqliteBannerRepository {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Banner>, CoreError> {
        let row = sqlx::query_as::<_, BannerRow>(
            "SELECT * FROM banners WHERE company_id = ?1 AND id = ?2 AND deleted_at IS NULL",
        )
        .bind(company_id.to_string())
        .bind(id.to_string())
        .fetch_optional(&self.pool).await.map_err(map_db)?;
        row.map(Banner::try_from).transpose()
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<Banner>, CoreError> {
        let rows = sqlx::query_as::<_, BannerRow>(
            "SELECT * FROM banners WHERE company_id = ?1 AND deleted_at IS NULL ORDER BY sort_order ASC, created_at DESC",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool).await.map_err(map_db)?;
        rows.into_iter().map(Banner::try_from).collect()
    }

    async fn find_active(&self, company_id: Uuid) -> Result<Vec<Banner>, CoreError> {
        let rows = sqlx::query_as::<_, BannerRow>(
            "SELECT * FROM banners WHERE company_id = ?1 AND deleted_at IS NULL AND active = 1 ORDER BY sort_order ASC, created_at DESC",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool).await.map_err(map_db)?;
        rows.into_iter().map(Banner::try_from).collect()
    }

    async fn create(&self, banner: &Banner) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO banners (id, company_id, title, image_data, item_type, item_id, item_url, active, sort_order, created_at, updated_at, deleted_at, synced)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        )
        .bind(banner.base.id.to_string())
        .bind(banner.base.company_id.to_string())
        .bind(&banner.title)
        .bind(&banner.image_data)
        .bind(&banner.item_type)
        .bind(banner.item_id.map(|u| u.to_string()))
        .bind(&banner.item_url)
        .bind(banner.active)
        .bind(banner.sort_order)
        .bind(ts(banner.base.created_at))
        .bind(ts(banner.base.updated_at))
        .bind(banner.base.deleted_at.map(ts))
        .bind(banner.base.synced)
        .execute(&self.pool).await.map_err(map_db)?;
        Ok(())
    }

    async fn update(&self, banner: &Banner) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE banners SET title = ?1, image_data = ?2, item_type = ?3, item_id = ?4, item_url = ?5, active = ?6, sort_order = ?7, updated_at = ?8, synced = ?9
             WHERE company_id = ?10 AND id = ?11 AND deleted_at IS NULL",
        )
        .bind(&banner.title)
        .bind(&banner.image_data)
        .bind(&banner.item_type)
        .bind(banner.item_id.map(|u| u.to_string()))
        .bind(&banner.item_url)
        .bind(banner.active)
        .bind(banner.sort_order)
        .bind(ts(banner.base.updated_at))
        .bind(banner.base.synced)
        .bind(banner.base.company_id.to_string())
        .bind(banner.base.id.to_string())
        .execute(&self.pool).await.map_err(map_db)?;
        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        sqlx::query(
            "UPDATE banners SET deleted_at = ?1, updated_at = ?2, synced = 0 WHERE company_id = ?3 AND id = ?4 AND deleted_at IS NULL",
        )
        .bind(ts(now)).bind(ts(now))
        .bind(company_id.to_string()).bind(id.to_string())
        .execute(&self.pool).await.map_err(map_db)?;
        Ok(())
    }

    async fn set_active(&self, company_id: Uuid, id: Uuid, active: bool) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        sqlx::query(
            "UPDATE banners SET active = ?1, updated_at = ?2, synced = 0 WHERE company_id = ?3 AND id = ?4 AND deleted_at IS NULL",
        )
        .bind(active).bind(ts(now))
        .bind(company_id.to_string()).bind(id.to_string())
        .execute(&self.pool).await.map_err(map_db)?;
        Ok(())
    }

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Banner>, CoreError> {
        let rows = sqlx::query_as::<_, BannerRow>(
            "SELECT * FROM banners WHERE company_id = ?1 AND synced = 0",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool).await.map_err(map_db)?;
        rows.into_iter().map(Banner::try_from).collect()
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query("UPDATE banners SET synced = 1 WHERE company_id = ?1 AND id = ?2 AND updated_at = ?3")
            .bind(company_id.to_string()).bind(id.to_string())
            .bind(ts(updated_at))
            .execute(&self.pool).await.map_err(map_db)?;
        Ok(())
    }

    async fn find_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<Banner>, CoreError> {
        let rows = sqlx::query_as::<_, BannerRow>(
            "SELECT * FROM banners WHERE company_id = ?1 AND updated_at > ?2",
        )
        .bind(company_id.to_string())
        .bind(ts(since))
        .fetch_all(&self.pool).await.map_err(map_db)?;
        rows.into_iter().map(Banner::try_from).collect()
    }

    async fn sync_upsert(&self, banner: &Banner) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO banners (id, company_id, title, image_data, item_type, item_id, item_url, active, sort_order, created_at, updated_at, deleted_at, synced)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT (id) DO UPDATE SET
                 title = excluded.title,
                 image_data = excluded.image_data,
                 item_type = excluded.item_type,
                 item_id = excluded.item_id,
                 item_url = excluded.item_url,
                 active = excluded.active,
                 sort_order = excluded.sort_order,
                 updated_at = excluded.updated_at,
                 deleted_at = excluded.deleted_at,
                 synced = excluded.synced
             WHERE excluded.updated_at > banners.updated_at",
        )
        .bind(banner.base.id.to_string())
        .bind(banner.base.company_id.to_string())
        .bind(&banner.title)
        .bind(&banner.image_data)
        .bind(&banner.item_type)
        .bind(banner.item_id.map(|u| u.to_string()))
        .bind(&banner.item_url)
        .bind(banner.active)
        .bind(banner.sort_order)
        .bind(ts(banner.base.created_at))
        .bind(ts(banner.base.updated_at))
        .bind(banner.base.deleted_at.map(ts))
        .bind(banner.base.synced)
        .execute(&self.pool).await.map_err(map_db)?;
        Ok(())
    }
}
