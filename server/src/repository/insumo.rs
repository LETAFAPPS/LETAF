//! Implementação PostgreSQL do `InsumoRepository`. Espelha
//! `PgProductRepository` no essencial: CRUD, ledger idempotente e sync
//! (keyset pull + upsert LWW sem sobrescrever o estoque).

use async_trait::async_trait;
use chrono::NaiveDateTime;
use rust_decimal::prelude::ToPrimitive;
use sqlx::prelude::FromRow;
use sqlx::PgPool;
use uuid::Uuid;

use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;
use letaf_core::insumo::model::{Insumo, InsumoMovement};
use letaf_core::insumo::repository::InsumoRepository;
use letaf_core::product::repository::StockAdjustResult;

use super::helpers::{keyset_pull_sql, map_db};

#[derive(FromRow)]
struct InsumoRow {
    id: Uuid,
    company_id: Uuid,
    name: String,
    description: Option<String>,
    unit: String,
    stock_quantity: f64,
    min_stock: f64,
    cost_price: Option<f64>,
    barcode: Option<String>,
    active: bool,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    deleted_at: Option<NaiveDateTime>,
    synced: bool,
}

impl From<InsumoRow> for Insumo {
    fn from(r: InsumoRow) -> Self {
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
            description: r.description,
            unit: r.unit,
            stock_quantity: r.stock_quantity,
            min_stock: r.min_stock,
            cost_price: r.cost_price.map(letaf_core::money::from_db_f64),
            barcode: r.barcode,
            active: r.active,
        }
    }
}

#[derive(FromRow)]
struct InsumoMovementRow {
    id: Uuid,
    company_id: Uuid,
    insumo_id: Uuid,
    delta: f64,
    reason: String,
    order_id: Option<Uuid>,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    deleted_at: Option<NaiveDateTime>,
    synced: bool,
}

impl From<InsumoMovementRow> for InsumoMovement {
    fn from(r: InsumoMovementRow) -> Self {
        Self {
            base: BaseFields {
                id: r.id,
                company_id: r.company_id,
                created_at: r.created_at,
                updated_at: r.updated_at,
                deleted_at: r.deleted_at,
                synced: r.synced,
            },
            insumo_id: r.insumo_id,
            delta: r.delta,
            reason: r.reason,
            order_id: r.order_id,
        }
    }
}

pub struct PgInsumoRepository {
    pool: PgPool,
}

impl PgInsumoRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const MV_COLS: &str = "id, company_id, insumo_id, delta, reason, order_id, created_at, updated_at, deleted_at, synced";

#[async_trait]
impl InsumoRepository for PgInsumoRepository {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Insumo>, CoreError> {
        let row = sqlx::query_as::<_, InsumoRow>(
            "SELECT * FROM insumos WHERE company_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(company_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(row.map(Insumo::from))
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<Insumo>, CoreError> {
        let rows = sqlx::query_as::<_, InsumoRow>(
            "SELECT * FROM insumos WHERE company_id = $1 AND deleted_at IS NULL ORDER BY name",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(Insumo::from).collect())
    }

    async fn create(&self, insumo: &Insumo) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO insumos (id, company_id, name, description, unit, stock_quantity, min_stock, cost_price, barcode, active, created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
        )
        .bind(insumo.base.id)
        .bind(insumo.base.company_id)
        .bind(&insumo.name)
        .bind(&insumo.description)
        .bind(&insumo.unit)
        .bind(insumo.stock_quantity)
        .bind(insumo.min_stock)
        .bind(insumo.cost_price.and_then(|d| d.to_f64()))
        .bind(&insumo.barcode)
        .bind(insumo.active)
        .bind(insumo.base.created_at)
        .bind(insumo.base.updated_at)
        .bind(insumo.base.deleted_at)
        .bind(insumo.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn update_atomic(&self, insumo: &Insumo, stock_delta: f64) -> Result<(), CoreError> {
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        sqlx::query(
            "UPDATE insumos SET name = $1, description = $2, unit = $3, stock_quantity = $4, min_stock = $5, cost_price = $6, barcode = $7, active = $8, updated_at = $9, synced = $10
             WHERE company_id = $11 AND id = $12 AND deleted_at IS NULL",
        )
        .bind(&insumo.name)
        .bind(&insumo.description)
        .bind(&insumo.unit)
        .bind(insumo.stock_quantity)
        .bind(insumo.min_stock)
        .bind(insumo.cost_price.and_then(|d| d.to_f64()))
        .bind(&insumo.barcode)
        .bind(insumo.active)
        .bind(insumo.base.updated_at)
        .bind(insumo.base.synced)
        .bind(insumo.base.company_id)
        .bind(insumo.base.id)
        .execute(&mut *tx)
        .await
        .map_err(map_db)?;

        if stock_delta.abs() > f64::EPSILON {
            let rows = sqlx::query(
                "UPDATE insumos SET stock_quantity = stock_quantity + $1, updated_at = $2, synced = false
                  WHERE company_id = $3 AND id = $4 AND deleted_at IS NULL AND stock_quantity + $1 >= 0",
            )
            .bind(stock_delta)
            .bind(insumo.base.updated_at)
            .bind(insumo.base.company_id)
            .bind(insumo.base.id)
            .execute(&mut *tx)
            .await
            .map_err(map_db)?
            .rows_affected();
            if rows != 1 {
                tx.rollback().await.map_err(map_db)?;
                return Err(CoreError::Validation("Estoque insuficiente para o ajuste".into()));
            }
            insert_mv(&mut tx, insumo.base.company_id, insumo.base.id, stock_delta, "edit", None, insumo.base.updated_at).await?;
        }
        tx.commit().await.map_err(map_db)?;
        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        sqlx::query(
            "UPDATE insumos SET deleted_at = $1, updated_at = $2, synced = false
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

    async fn try_adjust_stock(&self, company_id: Uuid, insumo_id: Uuid, delta: f64, reason: &str) -> Result<StockAdjustResult, CoreError> {
        let now = chrono::Utc::now().naive_utc();
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        let rows_affected = sqlx::query(
            "UPDATE insumos SET stock_quantity = stock_quantity + $1, updated_at = $2, synced = false
              WHERE company_id = $3 AND id = $4 AND deleted_at IS NULL AND stock_quantity + $1 >= 0",
        )
        .bind(delta)
        .bind(now)
        .bind(company_id)
        .bind(insumo_id)
        .execute(&mut *tx)
        .await
        .map_err(map_db)?
        .rows_affected();
        if rows_affected == 1 {
            insert_mv(&mut tx, company_id, insumo_id, delta, reason, None, now).await?;
            tx.commit().await.map_err(map_db)?;
            return Ok(StockAdjustResult::Adjusted);
        }
        tx.rollback().await.map_err(map_db)?;
        let row: Option<(Option<NaiveDateTime>,)> = sqlx::query_as(
            "SELECT deleted_at FROM insumos WHERE company_id = $1 AND id = $2",
        )
        .bind(company_id)
        .bind(insumo_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        match row {
            None => Ok(StockAdjustResult::NotFound),
            Some((Some(_),)) => Ok(StockAdjustResult::NotFound),
            Some((None,)) => Ok(StockAdjustResult::Insufficient),
        }
    }

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Insumo>, CoreError> {
        let rows = sqlx::query_as::<_, InsumoRow>(
            "SELECT * FROM insumos WHERE company_id = $1 AND synced = false",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(Insumo::from).collect())
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query("UPDATE insumos SET synced = true WHERE company_id = $1 AND id = $2 AND updated_at = $3")
            .bind(company_id)
            .bind(id)
            .bind(updated_at)
            .execute(&self.pool)
            .await
            .map_err(map_db)?;
        Ok(())
    }

    async fn sync_upsert(&self, insumo: &Insumo) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO insumos (id, company_id, name, description, unit, stock_quantity, min_stock, cost_price, barcode, active, created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
             ON CONFLICT (id) DO UPDATE SET
                 name = EXCLUDED.name,
                 description = EXCLUDED.description,
                 unit = EXCLUDED.unit,
                 -- stock_quantity NÃO é sobrescrito: o estoque evolui pelo ledger.
                 min_stock = EXCLUDED.min_stock,
                 cost_price = EXCLUDED.cost_price,
                 barcode = EXCLUDED.barcode,
                 active = EXCLUDED.active,
                 updated_at = EXCLUDED.updated_at,
                 deleted_at = EXCLUDED.deleted_at,
                 synced = EXCLUDED.synced
             WHERE EXCLUDED.updated_at > insumos.updated_at",
        )
        .bind(insumo.base.id)
        .bind(insumo.base.company_id)
        .bind(&insumo.name)
        .bind(&insumo.description)
        .bind(&insumo.unit)
        .bind(insumo.stock_quantity)
        .bind(insumo.min_stock)
        .bind(insumo.cost_price.and_then(|d| d.to_f64()))
        .bind(&insumo.barcode)
        .bind(insumo.active)
        .bind(insumo.base.created_at)
        .bind(insumo.base.updated_at)
        .bind(insumo.base.deleted_at)
        .bind(insumo.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn find_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<Insumo>, CoreError> {
        let rows = sqlx::query_as::<_, InsumoRow>(
            "SELECT * FROM insumos WHERE company_id = $1 AND updated_at > $2 ORDER BY updated_at ASC",
        )
        .bind(company_id)
        .bind(since)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(Insumo::from).collect())
    }

    async fn find_updated_since_paged(&self, company_id: Uuid, since: NaiveDateTime, after_id: Uuid, limit: i64) -> Result<Vec<Insumo>, CoreError> {
        let rows = sqlx::query_as::<_, InsumoRow>(&keyset_pull_sql("insumos"))
            .bind(company_id)
            .bind(since)
            .bind(after_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(map_db)?;
        Ok(rows.into_iter().map(Insumo::from).collect())
    }

    async fn find_unsynced_movements(&self, company_id: Uuid) -> Result<Vec<InsumoMovement>, CoreError> {
        let rows = sqlx::query_as::<_, InsumoMovementRow>(
            &format!("SELECT {MV_COLS} FROM insumo_movements WHERE company_id = $1 AND synced = false ORDER BY created_at ASC"),
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(InsumoMovement::from).collect())
    }

    async fn mark_movement_synced(&self, company_id: Uuid, id: Uuid, updated_at: NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query("UPDATE insumo_movements SET synced = true WHERE company_id = $1 AND id = $2 AND updated_at = $3")
            .bind(company_id)
            .bind(id)
            .bind(updated_at)
            .execute(&self.pool)
            .await
            .map_err(map_db)?;
        Ok(())
    }

    async fn apply_movement(&self, m: &InsumoMovement) -> Result<(), CoreError> {
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        let inserted = sqlx::query(
            "INSERT INTO insumo_movements (id, company_id, insumo_id, delta, reason, order_id, created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, true)
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(m.base.id)
        .bind(m.base.company_id)
        .bind(m.insumo_id)
        .bind(m.delta)
        .bind(&m.reason)
        .bind(m.order_id)
        .bind(m.base.created_at)
        .bind(m.base.updated_at)
        .bind(m.base.deleted_at)
        .execute(&mut *tx)
        .await
        .map_err(map_db)?
        .rows_affected();
        if inserted == 1 {
            sqlx::query(
                "UPDATE insumos
                    SET stock_quantity = stock_quantity + $1,
                        updated_at = GREATEST(insumos.updated_at, (now() AT TIME ZONE 'utc'))
                  WHERE company_id = $2 AND id = $3",
            )
            .bind(m.delta)
            .bind(m.base.company_id)
            .bind(m.insumo_id)
            .execute(&mut *tx)
            .await
            .map_err(map_db)?;
        }
        tx.commit().await.map_err(map_db)?;
        Ok(())
    }

    async fn insert_synced_movement(&self, m: &InsumoMovement) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO insumo_movements (id, company_id, insumo_id, delta, reason, order_id, created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, true)
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(m.base.id)
        .bind(m.base.company_id)
        .bind(m.insumo_id)
        .bind(m.delta)
        .bind(&m.reason)
        .bind(m.order_id)
        .bind(m.base.created_at)
        .bind(m.base.updated_at)
        .bind(m.base.deleted_at)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn find_movements_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<InsumoMovement>, CoreError> {
        let rows = sqlx::query_as::<_, InsumoMovementRow>(
            &format!("SELECT {MV_COLS} FROM insumo_movements WHERE company_id = $1 AND updated_at > $2 ORDER BY updated_at ASC"),
        )
        .bind(company_id)
        .bind(since)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(InsumoMovement::from).collect())
    }

    async fn find_movements_updated_since_paged(&self, company_id: Uuid, since: NaiveDateTime, after_id: Uuid, limit: i64) -> Result<Vec<InsumoMovement>, CoreError> {
        let rows = sqlx::query_as::<_, InsumoMovementRow>(&keyset_pull_sql("insumo_movements"))
            .bind(company_id)
            .bind(since)
            .bind(after_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(map_db)?;
        Ok(rows.into_iter().map(InsumoMovement::from).collect())
    }
}

/// Grava um movimento de insumo (ledger) dentro de uma transação aberta.
#[allow(clippy::too_many_arguments)]
async fn insert_mv(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    insumo_id: Uuid,
    delta: f64,
    reason: &str,
    order_id: Option<Uuid>,
    now: NaiveDateTime,
) -> Result<(), CoreError> {
    sqlx::query(
        "INSERT INTO insumo_movements (id, company_id, insumo_id, delta, reason, order_id, created_at, updated_at, deleted_at, synced)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $7, NULL, false)",
    )
    .bind(Uuid::new_v4())
    .bind(company_id)
    .bind(insumo_id)
    .bind(delta)
    .bind(reason)
    .bind(order_id)
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(map_db)?;
    Ok(())
}
