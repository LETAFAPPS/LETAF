use async_trait::async_trait;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::PgPool;
use uuid::Uuid;

use letaf_core::addon::model::Addon;
use letaf_core::addon::repository::AddonRepository;
use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;

use super::helpers::map_db;

#[derive(FromRow)]
struct AddonRow {
    id: Uuid,
    company_id: Uuid,
    group_id: Uuid,
    name: String,
    price: f64,
    sort_order: i32,
    active: bool,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    deleted_at: Option<NaiveDateTime>,
    synced: bool,
}

impl From<AddonRow> for Addon {
    fn from(r: AddonRow) -> Self {
        Self {
            base: BaseFields {
                id: r.id,
                company_id: r.company_id,
                created_at: r.created_at,
                updated_at: r.updated_at,
                deleted_at: r.deleted_at,
                synced: r.synced,
            },
            group_id: r.group_id,
            name: r.name,
            price: r.price,
            sort_order: r.sort_order,
            active: r.active,
        }
    }
}

pub struct PgAddonRepository {
    pool: PgPool,
}

impl PgAddonRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AddonRepository for PgAddonRepository {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Addon>, CoreError> {
        sqlx::query_as::<_, AddonRow>(
            "SELECT * FROM addons WHERE company_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(company_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map(|opt| opt.map(Addon::from))
        .map_err(map_db)
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<Addon>, CoreError> {
        let rows = sqlx::query_as::<_, AddonRow>(
            "SELECT * FROM addons WHERE company_id = $1 AND deleted_at IS NULL ORDER BY sort_order ASC, name ASC",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(Addon::from).collect())
    }

    async fn find_by_group(&self, company_id: Uuid, group_id: Uuid) -> Result<Vec<Addon>, CoreError> {
        let rows = sqlx::query_as::<_, AddonRow>(
            "SELECT * FROM addons
             WHERE company_id = $1 AND group_id = $2 AND deleted_at IS NULL
             ORDER BY sort_order ASC, name ASC",
        )
        .bind(company_id)
        .bind(group_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(Addon::from).collect())
    }

    async fn create(&self, a: &Addon) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO addons (id, company_id, group_id, name, price, sort_order, active, created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        )
        .bind(a.base.id)
        .bind(a.base.company_id)
        .bind(a.group_id)
        .bind(&a.name)
        .bind(a.price)
        .bind(a.sort_order)
        .bind(a.active)
        .bind(a.base.created_at)
        .bind(a.base.updated_at)
        .bind(a.base.deleted_at)
        .bind(a.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn update(&self, a: &Addon) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE addons SET group_id = $1, name = $2, price = $3, sort_order = $4, active = $5, updated_at = $6, synced = $7
             WHERE company_id = $8 AND id = $9 AND deleted_at IS NULL",
        )
        .bind(a.group_id)
        .bind(&a.name)
        .bind(a.price)
        .bind(a.sort_order)
        .bind(a.active)
        .bind(a.base.updated_at)
        .bind(a.base.synced)
        .bind(a.base.company_id)
        .bind(a.base.id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        sqlx::query(
            "UPDATE addons SET deleted_at = $1, updated_at = $2, synced = false
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

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Addon>, CoreError> {
        let rows = sqlx::query_as::<_, AddonRow>(
            "SELECT * FROM addons WHERE company_id = $1 AND synced = false",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(Addon::from).collect())
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        sqlx::query("UPDATE addons SET synced = true WHERE company_id = $1 AND id = $2")
            .bind(company_id)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(map_db)?;
        Ok(())
    }

    async fn find_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<Addon>, CoreError> {
        let rows = sqlx::query_as::<_, AddonRow>(
            "SELECT * FROM addons WHERE company_id = $1 AND updated_at > $2",
        )
        .bind(company_id)
        .bind(since)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(Addon::from).collect())
    }

    async fn sync_upsert(&self, a: &Addon) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO addons (id, company_id, group_id, name, price, sort_order, active, created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
             ON CONFLICT (id) DO UPDATE SET
                 group_id = EXCLUDED.group_id,
                 name = EXCLUDED.name,
                 price = EXCLUDED.price,
                 sort_order = EXCLUDED.sort_order,
                 active = EXCLUDED.active,
                 updated_at = EXCLUDED.updated_at,
                 deleted_at = EXCLUDED.deleted_at,
                 synced = EXCLUDED.synced
             WHERE EXCLUDED.updated_at > addons.updated_at AND addons.company_id = EXCLUDED.company_id",
        )
        .bind(a.base.id)
        .bind(a.base.company_id)
        .bind(a.group_id)
        .bind(&a.name)
        .bind(a.price)
        .bind(a.sort_order)
        .bind(a.active)
        .bind(a.base.created_at)
        .bind(a.base.updated_at)
        .bind(a.base.deleted_at)
        .bind(a.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }
}
