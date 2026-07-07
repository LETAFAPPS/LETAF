use async_trait::async_trait;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::SqlitePool;
use uuid::Uuid;

use letaf_core::addon_group::model::AddonGroup;
use letaf_core::addon_group::repository::AddonGroupRepository;
use letaf_core::error::CoreError;

use super::helpers::{parse_base, map_db, ts};

#[derive(FromRow)]
struct AddonGroupRow {
    id: String,
    company_id: String,
    name: String,
    selection: String,
    min_select: i32,
    max_select: i32,
    sort_order: i32,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    synced: bool,
}

impl TryFrom<AddonGroupRow> for AddonGroup {
    type Error = CoreError;

    fn try_from(r: AddonGroupRow) -> Result<Self, Self::Error> {
        Ok(Self {
            base: parse_base(&r.id, &r.company_id, &r.created_at, &r.updated_at, r.deleted_at.as_deref(), r.synced)?,
            name: r.name,
            selection: r.selection,
            min_select: r.min_select,
            max_select: r.max_select,
            sort_order: r.sort_order,
        })
    }
}

pub struct SqliteAddonGroupRepository {
    pool: SqlitePool,
}

impl SqliteAddonGroupRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AddonGroupRepository for SqliteAddonGroupRepository {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<AddonGroup>, CoreError> {
        let row = sqlx::query_as::<_, AddonGroupRow>(
            "SELECT * FROM addon_groups WHERE company_id = ?1 AND id = ?2 AND deleted_at IS NULL",
        )
        .bind(company_id.to_string())
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        row.map(AddonGroup::try_from).transpose()
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<AddonGroup>, CoreError> {
        let rows = sqlx::query_as::<_, AddonGroupRow>(
            "SELECT * FROM addon_groups WHERE company_id = ?1 AND deleted_at IS NULL ORDER BY sort_order ASC, name ASC",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(AddonGroup::try_from).collect()
    }

    async fn find_by_product(&self, company_id: Uuid, product_id: Uuid) -> Result<Vec<AddonGroup>, CoreError> {
        let rows = sqlx::query_as::<_, AddonGroupRow>(
            "SELECT g.* FROM addon_groups g
             INNER JOIN product_addon_groups pg
                 ON pg.group_id = g.id AND pg.company_id = g.company_id
             WHERE g.company_id = ?1 AND pg.product_id = ?2 AND g.deleted_at IS NULL
             ORDER BY pg.sort_order ASC, g.name ASC",
        )
        .bind(company_id.to_string())
        .bind(product_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(AddonGroup::try_from).collect()
    }

    async fn create(&self, g: &AddonGroup) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO addon_groups (id, company_id, name, selection, min_select, max_select, sort_order, created_at, updated_at, deleted_at, synced)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        )
        .bind(g.base.id.to_string())
        .bind(g.base.company_id.to_string())
        .bind(&g.name)
        .bind(&g.selection)
        .bind(g.min_select)
        .bind(g.max_select)
        .bind(g.sort_order)
        .bind(ts(g.base.created_at))
        .bind(ts(g.base.updated_at))
        .bind(g.base.deleted_at.map(ts))
        .bind(g.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn update(&self, g: &AddonGroup) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE addon_groups SET name = ?1, selection = ?2, min_select = ?3, max_select = ?4, sort_order = ?5, updated_at = ?6, synced = ?7
             WHERE company_id = ?8 AND id = ?9 AND deleted_at IS NULL",
        )
        .bind(&g.name)
        .bind(&g.selection)
        .bind(g.min_select)
        .bind(g.max_select)
        .bind(g.sort_order)
        .bind(ts(g.base.updated_at))
        .bind(g.base.synced)
        .bind(g.base.company_id.to_string())
        .bind(g.base.id.to_string())
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        sqlx::query(
            "UPDATE addon_groups SET deleted_at = ?1, updated_at = ?2, synced = false
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

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<AddonGroup>, CoreError> {
        let rows = sqlx::query_as::<_, AddonGroupRow>(
            "SELECT * FROM addon_groups WHERE company_id = ?1 AND synced = false",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(AddonGroup::try_from).collect()
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query("UPDATE addon_groups SET synced = true WHERE company_id = ?1 AND id = ?2 AND updated_at = ?3")
            .bind(company_id.to_string())
            .bind(id.to_string())
            .bind(ts(updated_at))
            .execute(&self.pool)
            .await
            .map_err(map_db)?;
        Ok(())
    }

    async fn find_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<AddonGroup>, CoreError> {
        let rows = sqlx::query_as::<_, AddonGroupRow>(
            "SELECT * FROM addon_groups WHERE company_id = ?1 AND updated_at > ?2",
        )
        .bind(company_id.to_string())
        .bind(ts(since))
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(AddonGroup::try_from).collect()
    }

    async fn sync_upsert(&self, g: &AddonGroup) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO addon_groups (id, company_id, name, selection, min_select, max_select, sort_order, created_at, updated_at, deleted_at, synced)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT (id) DO UPDATE SET
                 name = excluded.name,
                 selection = excluded.selection,
                 min_select = excluded.min_select,
                 max_select = excluded.max_select,
                 sort_order = excluded.sort_order,
                 updated_at = excluded.updated_at,
                 deleted_at = excluded.deleted_at,
                 synced = excluded.synced
             WHERE excluded.updated_at > addon_groups.updated_at",
        )
        .bind(g.base.id.to_string())
        .bind(g.base.company_id.to_string())
        .bind(&g.name)
        .bind(&g.selection)
        .bind(g.min_select)
        .bind(g.max_select)
        .bind(g.sort_order)
        .bind(ts(g.base.created_at))
        .bind(ts(g.base.updated_at))
        .bind(g.base.deleted_at.map(ts))
        .bind(g.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }
}
