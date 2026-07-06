use async_trait::async_trait;
use rust_decimal::prelude::ToPrimitive;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::SqlitePool;
use uuid::Uuid;

use letaf_core::addon::model::Addon;
use letaf_core::addon::repository::AddonRepository;
use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;

use super::helpers::{map_db, parse_timestamp, parse_uuid, ts};

#[derive(FromRow)]
struct AddonRow {
    id: String,
    company_id: String,
    group_id: String,
    name: String,
    price: f64,
    sort_order: i32,
    active: bool,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    synced: bool,
}

impl TryFrom<AddonRow> for Addon {
    type Error = CoreError;

    fn try_from(r: AddonRow) -> Result<Self, Self::Error> {
        Ok(Self {
            base: BaseFields {
                id: parse_uuid(&r.id)?,
                company_id: parse_uuid(&r.company_id)?,
                created_at: parse_timestamp(&r.created_at)?,
                updated_at: parse_timestamp(&r.updated_at)?,
                deleted_at: r.deleted_at.as_deref().map(parse_timestamp).transpose()?,
                synced: r.synced,
            },
            group_id: parse_uuid(&r.group_id)?,
            name: r.name,
            price: letaf_core::money::from_db_f64(r.price),
            sort_order: r.sort_order,
            active: r.active,
        })
    }
}

pub struct SqliteAddonRepository {
    pool: SqlitePool,
}

impl SqliteAddonRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AddonRepository for SqliteAddonRepository {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Addon>, CoreError> {
        let row = sqlx::query_as::<_, AddonRow>(
            "SELECT * FROM addons WHERE company_id = ?1 AND id = ?2 AND deleted_at IS NULL",
        )
        .bind(company_id.to_string())
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        row.map(Addon::try_from).transpose()
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<Addon>, CoreError> {
        let rows = sqlx::query_as::<_, AddonRow>(
            "SELECT * FROM addons WHERE company_id = ?1 AND deleted_at IS NULL ORDER BY sort_order ASC, name ASC",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(Addon::try_from).collect()
    }

    async fn find_by_group(&self, company_id: Uuid, group_id: Uuid) -> Result<Vec<Addon>, CoreError> {
        let rows = sqlx::query_as::<_, AddonRow>(
            "SELECT * FROM addons
             WHERE company_id = ?1 AND group_id = ?2 AND deleted_at IS NULL
             ORDER BY sort_order ASC, name ASC",
        )
        .bind(company_id.to_string())
        .bind(group_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(Addon::try_from).collect()
    }

    async fn create(&self, a: &Addon) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO addons (id, company_id, group_id, name, price, sort_order, active, created_at, updated_at, deleted_at, synced)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        )
        .bind(a.base.id.to_string())
        .bind(a.base.company_id.to_string())
        .bind(a.group_id.to_string())
        .bind(&a.name)
        .bind(a.price.to_f64().unwrap_or(0.0))
        .bind(a.sort_order)
        .bind(a.active)
        .bind(ts(a.base.created_at))
        .bind(ts(a.base.updated_at))
        .bind(a.base.deleted_at.map(ts))
        .bind(a.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn update(&self, a: &Addon) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE addons SET group_id = ?1, name = ?2, price = ?3, sort_order = ?4, active = ?5, updated_at = ?6, synced = ?7
             WHERE company_id = ?8 AND id = ?9 AND deleted_at IS NULL",
        )
        .bind(a.group_id.to_string())
        .bind(&a.name)
        .bind(a.price.to_f64().unwrap_or(0.0))
        .bind(a.sort_order)
        .bind(a.active)
        .bind(ts(a.base.updated_at))
        .bind(a.base.synced)
        .bind(a.base.company_id.to_string())
        .bind(a.base.id.to_string())
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        sqlx::query(
            "UPDATE addons SET deleted_at = ?1, updated_at = ?2, synced = false
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

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Addon>, CoreError> {
        let rows = sqlx::query_as::<_, AddonRow>(
            "SELECT * FROM addons WHERE company_id = ?1 AND synced = false",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(Addon::try_from).collect()
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query("UPDATE addons SET synced = true WHERE company_id = ?1 AND id = ?2 AND updated_at = ?3")
            .bind(company_id.to_string())
            .bind(id.to_string())
            .bind(ts(updated_at))
            .execute(&self.pool)
            .await
            .map_err(map_db)?;
        Ok(())
    }

    async fn find_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<Addon>, CoreError> {
        let rows = sqlx::query_as::<_, AddonRow>(
            "SELECT * FROM addons WHERE company_id = ?1 AND updated_at > ?2",
        )
        .bind(company_id.to_string())
        .bind(ts(since))
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(Addon::try_from).collect()
    }

    async fn sync_upsert(&self, a: &Addon) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO addons (id, company_id, group_id, name, price, sort_order, active, created_at, updated_at, deleted_at, synced)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT (id) DO UPDATE SET
                 group_id = excluded.group_id,
                 name = excluded.name,
                 price = excluded.price,
                 sort_order = excluded.sort_order,
                 active = excluded.active,
                 updated_at = excluded.updated_at,
                 deleted_at = excluded.deleted_at,
                 synced = excluded.synced
             WHERE excluded.updated_at > addons.updated_at",
        )
        .bind(a.base.id.to_string())
        .bind(a.base.company_id.to_string())
        .bind(a.group_id.to_string())
        .bind(&a.name)
        .bind(a.price.to_f64().unwrap_or(0.0))
        .bind(a.sort_order)
        .bind(a.active)
        .bind(ts(a.base.created_at))
        .bind(ts(a.base.updated_at))
        .bind(a.base.deleted_at.map(ts))
        .bind(a.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }
}
