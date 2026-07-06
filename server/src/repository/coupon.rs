use async_trait::async_trait;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::PgPool;
use uuid::Uuid;

use letaf_core::coupon::model::Coupon;
use letaf_core::coupon::repository::CouponRepository;
use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;

use super::helpers::map_db;

#[derive(FromRow)]
struct CouponRow {
    id: Uuid,
    company_id: Uuid,
    title: String,
    code: String,
    coupon_type: String,
    discount_kind: String,
    discount_value: f64,
    min_order_value: f64,
    max_discount: f64,
    per_user_limit: i32,
    usage_limit: i32,
    valid_from: Option<NaiveDateTime>,
    valid_until: Option<NaiveDateTime>,
    active: bool,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    deleted_at: Option<NaiveDateTime>,
    synced: bool,
}

impl From<CouponRow> for Coupon {
    fn from(r: CouponRow) -> Self {
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
            code: r.code,
            coupon_type: r.coupon_type,
            discount_kind: r.discount_kind,
            discount_value: r.discount_value,
            min_order_value: r.min_order_value,
            max_discount: r.max_discount,
            per_user_limit: r.per_user_limit,
            usage_limit: r.usage_limit,
            valid_from: r.valid_from,
            valid_until: r.valid_until,
            active: r.active,
        }
    }
}

pub struct PgCouponRepository {
    pool: PgPool,
}

impl PgCouponRepository {
    pub fn new(pool: PgPool) -> Self { Self { pool } }
}

#[async_trait]
impl CouponRepository for PgCouponRepository {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Coupon>, CoreError> {
        sqlx::query_as::<_, CouponRow>(
            "SELECT * FROM coupons WHERE company_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(company_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map(|opt| opt.map(Coupon::from))
        .map_err(map_db)
    }

    async fn find_by_code(&self, company_id: Uuid, code: &str) -> Result<Option<Coupon>, CoreError> {
        sqlx::query_as::<_, CouponRow>(
            "SELECT * FROM coupons WHERE company_id = $1 AND code = $2 AND deleted_at IS NULL",
        )
        .bind(company_id)
        .bind(code)
        .fetch_optional(&self.pool)
        .await
        .map(|opt| opt.map(Coupon::from))
        .map_err(map_db)
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<Coupon>, CoreError> {
        let rows = sqlx::query_as::<_, CouponRow>(
            "SELECT * FROM coupons WHERE company_id = $1 AND deleted_at IS NULL ORDER BY created_at DESC",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(Coupon::from).collect())
    }

    async fn find_active(&self, company_id: Uuid) -> Result<Vec<Coupon>, CoreError> {
        let rows = sqlx::query_as::<_, CouponRow>(
            "SELECT * FROM coupons WHERE company_id = $1 AND deleted_at IS NULL AND active = TRUE ORDER BY created_at DESC",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(Coupon::from).collect())
    }

    async fn create(&self, c: &Coupon) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO coupons (id, company_id, title, code, coupon_type, discount_kind, discount_value, min_order_value, max_discount, per_user_limit, usage_limit, valid_from, valid_until, active, created_at, updated_at, deleted_at, synced)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18)",
        )
        .bind(c.base.id)
        .bind(c.base.company_id)
        .bind(&c.title)
        .bind(&c.code)
        .bind(&c.coupon_type)
        .bind(&c.discount_kind)
        .bind(c.discount_value)
        .bind(c.min_order_value)
        .bind(c.max_discount)
        .bind(c.per_user_limit)
        .bind(c.usage_limit)
        .bind(c.valid_from)
        .bind(c.valid_until)
        .bind(c.active)
        .bind(c.base.created_at)
        .bind(c.base.updated_at)
        .bind(c.base.deleted_at)
        .bind(c.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn update(&self, c: &Coupon) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE coupons SET title=$1, code=$2, coupon_type=$3, discount_kind=$4, discount_value=$5, min_order_value=$6, max_discount=$7, per_user_limit=$8, usage_limit=$9, valid_from=$10, valid_until=$11, active=$12, updated_at=$13, synced=$14
             WHERE company_id=$15 AND id=$16 AND deleted_at IS NULL",
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
        .bind(c.valid_from)
        .bind(c.valid_until)
        .bind(c.active)
        .bind(c.base.updated_at)
        .bind(c.base.synced)
        .bind(c.base.company_id)
        .bind(c.base.id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        sqlx::query(
            "UPDATE coupons SET deleted_at=$1, updated_at=$2, synced=false WHERE company_id=$3 AND id=$4 AND deleted_at IS NULL",
        )
        .bind(now).bind(now).bind(company_id).bind(id)
        .execute(&self.pool).await.map_err(map_db)?;
        Ok(())
    }

    async fn set_active(&self, company_id: Uuid, id: Uuid, active: bool) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        sqlx::query(
            "UPDATE coupons SET active=$1, updated_at=$2, synced=false WHERE company_id=$3 AND id=$4 AND deleted_at IS NULL",
        )
        .bind(active).bind(now).bind(company_id).bind(id)
        .execute(&self.pool).await.map_err(map_db)?;
        Ok(())
    }

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Coupon>, CoreError> {
        let rows = sqlx::query_as::<_, CouponRow>(
            "SELECT * FROM coupons WHERE company_id = $1 AND synced = false",
        )
        .bind(company_id).fetch_all(&self.pool).await.map_err(map_db)?;
        Ok(rows.into_iter().map(Coupon::from).collect())
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        sqlx::query("UPDATE coupons SET synced = true WHERE company_id = $1 AND id = $2")
            .bind(company_id).bind(id)
            .execute(&self.pool).await.map_err(map_db)?;
        Ok(())
    }

    async fn find_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<Coupon>, CoreError> {
        let rows = sqlx::query_as::<_, CouponRow>(
            "SELECT * FROM coupons WHERE company_id = $1 AND updated_at > $2",
        )
        .bind(company_id).bind(since).fetch_all(&self.pool).await.map_err(map_db)?;
        Ok(rows.into_iter().map(Coupon::from).collect())
    }

    async fn sync_upsert(&self, c: &Coupon) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO coupons (id, company_id, title, code, coupon_type, discount_kind, discount_value, min_order_value, max_discount, per_user_limit, usage_limit, valid_from, valid_until, active, created_at, updated_at, deleted_at, synced)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18)
             ON CONFLICT (id) DO UPDATE SET
                 title = EXCLUDED.title,
                 code = EXCLUDED.code,
                 coupon_type = EXCLUDED.coupon_type,
                 discount_kind = EXCLUDED.discount_kind,
                 discount_value = EXCLUDED.discount_value,
                 min_order_value = EXCLUDED.min_order_value,
                 max_discount = EXCLUDED.max_discount,
                 per_user_limit = EXCLUDED.per_user_limit,
                 usage_limit = EXCLUDED.usage_limit,
                 valid_from = EXCLUDED.valid_from,
                 valid_until = EXCLUDED.valid_until,
                 active = EXCLUDED.active,
                 updated_at = EXCLUDED.updated_at,
                 deleted_at = EXCLUDED.deleted_at,
                 synced = EXCLUDED.synced
             WHERE EXCLUDED.updated_at > coupons.updated_at AND coupons.company_id = EXCLUDED.company_id",
        )
        .bind(c.base.id)
        .bind(c.base.company_id)
        .bind(&c.title)
        .bind(&c.code)
        .bind(&c.coupon_type)
        .bind(&c.discount_kind)
        .bind(c.discount_value)
        .bind(c.min_order_value)
        .bind(c.max_discount)
        .bind(c.per_user_limit)
        .bind(c.usage_limit)
        .bind(c.valid_from)
        .bind(c.valid_until)
        .bind(c.active)
        .bind(c.base.created_at)
        .bind(c.base.updated_at)
        .bind(c.base.deleted_at)
        .bind(c.base.synced)
        .execute(&self.pool).await.map_err(map_db)?;
        Ok(())
    }
}
