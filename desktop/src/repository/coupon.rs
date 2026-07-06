use async_trait::async_trait;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::SqlitePool;
use uuid::Uuid;

use letaf_core::coupon::model::Coupon;
use letaf_core::coupon::repository::CouponRepository;
use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;

use super::helpers::{map_db, parse_timestamp, parse_uuid, ts};

#[derive(FromRow)]
struct CouponRow {
    id: String,
    company_id: String,
    title: String,
    code: String,
    coupon_type: String,
    discount_kind: String,
    discount_value: f64,
    min_order_value: f64,
    max_discount: f64,
    per_user_limit: i32,
    usage_limit: i32,
    valid_from: Option<String>,
    valid_until: Option<String>,
    active: bool,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    synced: bool,
}

impl TryFrom<CouponRow> for Coupon {
    type Error = CoreError;
    fn try_from(r: CouponRow) -> Result<Self, Self::Error> {
        Ok(Self {
            base: BaseFields {
                id: parse_uuid(&r.id)?,
                company_id: parse_uuid(&r.company_id)?,
                created_at: parse_timestamp(&r.created_at)?,
                updated_at: parse_timestamp(&r.updated_at)?,
                deleted_at: r.deleted_at.as_deref().map(parse_timestamp).transpose()?,
                synced: r.synced,
            },
            title: r.title,
            code: r.code,
            coupon_type: r.coupon_type,
            discount_kind: r.discount_kind,
            discount_value: r.discount_value,
            min_order_value: r.min_order_value,
            max_discount: r.max_discount,
            per_user_limit: r.per_user_limit,
            usage_limit: r.usage_limit,
            valid_from: r.valid_from.as_deref().map(parse_timestamp).transpose()?,
            valid_until: r.valid_until.as_deref().map(parse_timestamp).transpose()?,
            active: r.active,
        })
    }
}

pub struct SqliteCouponRepository {
    pool: SqlitePool,
}

impl SqliteCouponRepository {
    pub fn new(pool: SqlitePool) -> Self { Self { pool } }
}

#[async_trait]
impl CouponRepository for SqliteCouponRepository {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Coupon>, CoreError> {
        let row = sqlx::query_as::<_, CouponRow>(
            "SELECT * FROM coupons WHERE company_id = ?1 AND id = ?2 AND deleted_at IS NULL",
        )
        .bind(company_id.to_string())
        .bind(id.to_string())
        .fetch_optional(&self.pool).await.map_err(map_db)?;
        row.map(Coupon::try_from).transpose()
    }

    async fn find_by_code(&self, company_id: Uuid, code: &str) -> Result<Option<Coupon>, CoreError> {
        let row = sqlx::query_as::<_, CouponRow>(
            "SELECT * FROM coupons WHERE company_id = ?1 AND code = ?2 AND deleted_at IS NULL",
        )
        .bind(company_id.to_string())
        .bind(code)
        .fetch_optional(&self.pool).await.map_err(map_db)?;
        row.map(Coupon::try_from).transpose()
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<Coupon>, CoreError> {
        let rows = sqlx::query_as::<_, CouponRow>(
            "SELECT * FROM coupons WHERE company_id = ?1 AND deleted_at IS NULL ORDER BY created_at DESC",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool).await.map_err(map_db)?;
        rows.into_iter().map(Coupon::try_from).collect()
    }

    async fn find_active(&self, company_id: Uuid) -> Result<Vec<Coupon>, CoreError> {
        let rows = sqlx::query_as::<_, CouponRow>(
            "SELECT * FROM coupons WHERE company_id = ?1 AND deleted_at IS NULL AND active = 1 ORDER BY created_at DESC",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool).await.map_err(map_db)?;
        rows.into_iter().map(Coupon::try_from).collect()
    }

    async fn create(&self, c: &Coupon) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO coupons (id, company_id, title, code, coupon_type, discount_kind, discount_value, min_order_value, max_discount, per_user_limit, usage_limit, valid_from, valid_until, active, created_at, updated_at, deleted_at, synced)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
        )
        .bind(c.base.id.to_string())
        .bind(c.base.company_id.to_string())
        .bind(&c.title)
        .bind(&c.code)
        .bind(&c.coupon_type)
        .bind(&c.discount_kind)
        .bind(c.discount_value)
        .bind(c.min_order_value)
        .bind(c.max_discount)
        .bind(c.per_user_limit)
        .bind(c.usage_limit)
        .bind(c.valid_from.map(ts))
        .bind(c.valid_until.map(ts))
        .bind(c.active)
        .bind(ts(c.base.created_at))
        .bind(ts(c.base.updated_at))
        .bind(c.base.deleted_at.map(ts))
        .bind(c.base.synced)
        .execute(&self.pool).await.map_err(map_db)?;
        Ok(())
    }

    async fn update(&self, c: &Coupon) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE coupons SET title=?1, code=?2, coupon_type=?3, discount_kind=?4, discount_value=?5, min_order_value=?6, max_discount=?7, per_user_limit=?8, usage_limit=?9, valid_from=?10, valid_until=?11, active=?12, updated_at=?13, synced=?14
             WHERE company_id=?15 AND id=?16 AND deleted_at IS NULL",
        )
        .bind(&c.title)
        .bind(&c.code)
        .bind(&c.coupon_type)
        .bind(&c.discount_kind)
        .bind(c.discount_value)
        .bind(c.min_order_value)
        .bind(c.max_discount)
        .bind(c.per_user_limit)
        .bind(c.usage_limit)
        .bind(c.valid_from.map(ts))
        .bind(c.valid_until.map(ts))
        .bind(c.active)
        .bind(ts(c.base.updated_at))
        .bind(c.base.synced)
        .bind(c.base.company_id.to_string())
        .bind(c.base.id.to_string())
        .execute(&self.pool).await.map_err(map_db)?;
        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        sqlx::query(
            "UPDATE coupons SET deleted_at = ?1, updated_at = ?2, synced = 0 WHERE company_id = ?3 AND id = ?4 AND deleted_at IS NULL",
        )
        .bind(ts(now)).bind(ts(now))
        .bind(company_id.to_string()).bind(id.to_string())
        .execute(&self.pool).await.map_err(map_db)?;
        Ok(())
    }

    async fn set_active(&self, company_id: Uuid, id: Uuid, active: bool) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        sqlx::query(
            "UPDATE coupons SET active = ?1, updated_at = ?2, synced = 0 WHERE company_id = ?3 AND id = ?4 AND deleted_at IS NULL",
        )
        .bind(active).bind(ts(now))
        .bind(company_id.to_string()).bind(id.to_string())
        .execute(&self.pool).await.map_err(map_db)?;
        Ok(())
    }

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Coupon>, CoreError> {
        let rows = sqlx::query_as::<_, CouponRow>(
            "SELECT * FROM coupons WHERE company_id = ?1 AND synced = 0",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool).await.map_err(map_db)?;
        rows.into_iter().map(Coupon::try_from).collect()
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query("UPDATE coupons SET synced = 1 WHERE company_id = ?1 AND id = ?2 AND updated_at = ?3")
            .bind(company_id.to_string()).bind(id.to_string())
            .bind(ts(updated_at))
            .execute(&self.pool).await.map_err(map_db)?;
        Ok(())
    }

    async fn find_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<Coupon>, CoreError> {
        let rows = sqlx::query_as::<_, CouponRow>(
            "SELECT * FROM coupons WHERE company_id = ?1 AND updated_at > ?2",
        )
        .bind(company_id.to_string())
        .bind(ts(since))
        .fetch_all(&self.pool).await.map_err(map_db)?;
        rows.into_iter().map(Coupon::try_from).collect()
    }

    async fn sync_upsert(&self, c: &Coupon) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO coupons (id, company_id, title, code, coupon_type, discount_kind, discount_value, min_order_value, max_discount, per_user_limit, usage_limit, valid_from, valid_until, active, created_at, updated_at, deleted_at, synced)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)
             ON CONFLICT (id) DO UPDATE SET
                 title = excluded.title,
                 code = excluded.code,
                 coupon_type = excluded.coupon_type,
                 discount_kind = excluded.discount_kind,
                 discount_value = excluded.discount_value,
                 min_order_value = excluded.min_order_value,
                 max_discount = excluded.max_discount,
                 per_user_limit = excluded.per_user_limit,
                 usage_limit = excluded.usage_limit,
                 valid_from = excluded.valid_from,
                 valid_until = excluded.valid_until,
                 active = excluded.active,
                 updated_at = excluded.updated_at,
                 deleted_at = excluded.deleted_at,
                 synced = excluded.synced
             WHERE excluded.updated_at > coupons.updated_at",
        )
        .bind(c.base.id.to_string())
        .bind(c.base.company_id.to_string())
        .bind(&c.title)
        .bind(&c.code)
        .bind(&c.coupon_type)
        .bind(&c.discount_kind)
        .bind(c.discount_value)
        .bind(c.min_order_value)
        .bind(c.max_discount)
        .bind(c.per_user_limit)
        .bind(c.usage_limit)
        .bind(c.valid_from.map(ts))
        .bind(c.valid_until.map(ts))
        .bind(c.active)
        .bind(ts(c.base.created_at))
        .bind(ts(c.base.updated_at))
        .bind(c.base.deleted_at.map(ts))
        .bind(c.base.synced)
        .execute(&self.pool).await.map_err(map_db)?;
        Ok(())
    }
}
