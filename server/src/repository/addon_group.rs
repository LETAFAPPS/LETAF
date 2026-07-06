use async_trait::async_trait;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::PgPool;
use uuid::Uuid;

use letaf_core::addon_group::model::AddonGroup;
use letaf_core::addon_group::repository::AddonGroupRepository;
use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;

use super::helpers::map_db;

#[derive(FromRow)]
struct AddonGroupRow {
    id: Uuid,
    company_id: Uuid,
    name: String,
    selection: String,
    min_select: i32,
    max_select: i32,
    sort_order: i32,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    deleted_at: Option<NaiveDateTime>,
    synced: bool,
}

impl From<AddonGroupRow> for AddonGroup {
    fn from(r: AddonGroupRow) -> Self {
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
            selection: r.selection,
            min_select: r.min_select,
            max_select: r.max_select,
            sort_order: r.sort_order,
        }
    }
}

/// Implementação PostgreSQL do AddonGroupRepository.
///
/// Regras aplicadas (AI_RULES.md §3, §5, §10, §11):
/// - Todas queries filtram por company_id (isolamento).
/// - Soft delete via deleted_at.
/// - Acesso ao banco somente via repository.
pub struct PgAddonGroupRepository {
    pool: PgPool,
}

impl PgAddonGroupRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AddonGroupRepository for PgAddonGroupRepository {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<AddonGroup>, CoreError> {
        sqlx::query_as::<_, AddonGroupRow>(
            "SELECT * FROM addon_groups WHERE company_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(company_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map(|opt| opt.map(AddonGroup::from))
        .map_err(map_db)
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<AddonGroup>, CoreError> {
        let rows = sqlx::query_as::<_, AddonGroupRow>(
            "SELECT * FROM addon_groups WHERE company_id = $1 AND deleted_at IS NULL ORDER BY sort_order ASC, name ASC",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(AddonGroup::from).collect())
    }

    async fn find_by_product(&self, company_id: Uuid, product_id: Uuid) -> Result<Vec<AddonGroup>, CoreError> {
        let rows = sqlx::query_as::<_, AddonGroupRow>(
            "SELECT g.* FROM addon_groups g
             INNER JOIN product_addon_groups pg
                 ON pg.group_id = g.id AND pg.company_id = g.company_id
             WHERE g.company_id = $1 AND pg.product_id = $2 AND g.deleted_at IS NULL
             ORDER BY pg.sort_order ASC, g.name ASC",
        )
        .bind(company_id)
        .bind(product_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(AddonGroup::from).collect())
    }

    async fn create(&self, g: &AddonGroup) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO addon_groups (id, company_id, name, selection, min_select, max_select, sort_order, created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        )
        .bind(g.base.id)
        .bind(g.base.company_id)
        .bind(&g.name)
        .bind(&g.selection)
        .bind(g.min_select)
        .bind(g.max_select)
        .bind(g.sort_order)
        .bind(g.base.created_at)
        .bind(g.base.updated_at)
        .bind(g.base.deleted_at)
        .bind(g.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn update(&self, g: &AddonGroup) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE addon_groups SET name = $1, selection = $2, min_select = $3, max_select = $4, sort_order = $5, updated_at = $6, synced = $7
             WHERE company_id = $8 AND id = $9 AND deleted_at IS NULL",
        )
        .bind(&g.name)
        .bind(&g.selection)
        .bind(g.min_select)
        .bind(g.max_select)
        .bind(g.sort_order)
        .bind(g.base.updated_at)
        .bind(g.base.synced)
        .bind(g.base.company_id)
        .bind(g.base.id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        sqlx::query(
            "UPDATE addon_groups SET deleted_at = $1, updated_at = $2, synced = false
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

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<AddonGroup>, CoreError> {
        let rows = sqlx::query_as::<_, AddonGroupRow>(
            "SELECT * FROM addon_groups WHERE company_id = $1 AND synced = false",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(AddonGroup::from).collect())
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query("UPDATE addon_groups SET synced = true WHERE company_id = $1 AND id = $2 AND updated_at = $3")
            .bind(company_id)
            .bind(id)
        .bind(updated_at)
            .execute(&self.pool)
            .await
            .map_err(map_db)?;
        Ok(())
    }

    async fn find_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<AddonGroup>, CoreError> {
        let rows = sqlx::query_as::<_, AddonGroupRow>(
            "SELECT * FROM addon_groups WHERE company_id = $1 AND updated_at > $2",
        )
        .bind(company_id)
        .bind(since)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(AddonGroup::from).collect())
    }

    async fn sync_upsert(&self, g: &AddonGroup) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO addon_groups (id, company_id, name, selection, min_select, max_select, sort_order, created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
             ON CONFLICT (id) DO UPDATE SET
                 name = EXCLUDED.name,
                 selection = EXCLUDED.selection,
                 min_select = EXCLUDED.min_select,
                 max_select = EXCLUDED.max_select,
                 sort_order = EXCLUDED.sort_order,
                 updated_at = EXCLUDED.updated_at,
                 deleted_at = EXCLUDED.deleted_at,
                 synced = EXCLUDED.synced
             WHERE EXCLUDED.updated_at > addon_groups.updated_at AND addon_groups.company_id = EXCLUDED.company_id",
        )
        .bind(g.base.id)
        .bind(g.base.company_id)
        .bind(&g.name)
        .bind(&g.selection)
        .bind(g.min_select)
        .bind(g.max_select)
        .bind(g.sort_order)
        .bind(g.base.created_at)
        .bind(g.base.updated_at)
        .bind(g.base.deleted_at)
        .bind(g.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }
}
