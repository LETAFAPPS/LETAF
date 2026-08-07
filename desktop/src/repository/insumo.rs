//! Implementação SQLite do `InsumoRepository` (desktop, offline-first).
//! Espelha o essencial de `SqliteProductRepository`: CRUD, ledger de
//! movimentos e sync (push/pull + upsert LWW sem sobrescrever o estoque).

use async_trait::async_trait;
use rust_decimal::prelude::ToPrimitive;
use sqlx::prelude::FromRow;
use sqlx::SqlitePool;
use uuid::Uuid;

use letaf_core::error::CoreError;
use letaf_core::insumo::model::{Insumo, InsumoMovement};
use letaf_core::insumo::repository::InsumoRepository;
use letaf_core::product::repository::StockAdjustResult;

use super::helpers::{map_db, parse_base, parse_uuid, ts};

#[derive(FromRow)]
struct InsumoRow {
    id: String,
    company_id: String,
    name: String,
    description: Option<String>,
    unit: String,
    stock_quantity: f64,
    min_stock: f64,
    cost_price: Option<f64>,
    barcode: Option<String>,
    active: bool,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    synced: bool,
}

impl TryFrom<InsumoRow> for Insumo {
    type Error = CoreError;
    fn try_from(r: InsumoRow) -> Result<Self, Self::Error> {
        Ok(Self {
            base: parse_base(&r.id, &r.company_id, &r.created_at, &r.updated_at, r.deleted_at.as_deref(), r.synced)?,
            name: r.name,
            description: r.description,
            unit: r.unit,
            stock_quantity: r.stock_quantity,
            min_stock: r.min_stock,
            cost_price: r.cost_price.map(letaf_core::money::from_db_f64),
            barcode: r.barcode,
            active: r.active,
        })
    }
}

#[derive(FromRow)]
struct InsumoMovementRow {
    id: String,
    company_id: String,
    insumo_id: String,
    delta: f64,
    reason: String,
    order_id: Option<String>,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    synced: bool,
}

impl TryFrom<InsumoMovementRow> for InsumoMovement {
    type Error = CoreError;
    fn try_from(r: InsumoMovementRow) -> Result<Self, Self::Error> {
        Ok(Self {
            base: parse_base(&r.id, &r.company_id, &r.created_at, &r.updated_at, r.deleted_at.as_deref(), r.synced)?,
            insumo_id: parse_uuid(&r.insumo_id)?,
            delta: r.delta,
            reason: r.reason,
            order_id: r.order_id.as_deref().map(parse_uuid).transpose()?,
        })
    }
}

pub struct SqliteInsumoRepository {
    pool: SqlitePool,
}

impl SqliteInsumoRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

const MV_COLS: &str = "id, company_id, insumo_id, delta, reason, order_id, created_at, updated_at, deleted_at, synced";

#[async_trait]
impl InsumoRepository for SqliteInsumoRepository {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Insumo>, CoreError> {
        let row = sqlx::query_as::<_, InsumoRow>(
            "SELECT * FROM insumos WHERE company_id = ?1 AND id = ?2 AND deleted_at IS NULL",
        )
        .bind(company_id.to_string())
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        row.map(Insumo::try_from).transpose()
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<Insumo>, CoreError> {
        let rows = sqlx::query_as::<_, InsumoRow>(
            "SELECT * FROM insumos WHERE company_id = ?1 AND deleted_at IS NULL ORDER BY name",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(Insumo::try_from).collect()
    }

    async fn create(&self, insumo: &Insumo) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO insumos (id, company_id, name, description, unit, stock_quantity, min_stock, cost_price, barcode, active, created_at, updated_at, deleted_at, synced)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        )
        .bind(insumo.base.id.to_string())
        .bind(insumo.base.company_id.to_string())
        .bind(&insumo.name)
        .bind(&insumo.description)
        .bind(&insumo.unit)
        .bind(insumo.stock_quantity)
        .bind(insumo.min_stock)
        .bind(insumo.cost_price.and_then(|d| d.to_f64()))
        .bind(&insumo.barcode)
        .bind(insumo.active)
        .bind(ts(insumo.base.created_at))
        .bind(ts(insumo.base.updated_at))
        .bind(insumo.base.deleted_at.map(ts))
        .bind(insumo.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn update_atomic(&self, insumo: &Insumo, stock_delta: f64) -> Result<(), CoreError> {
        let updated = ts(insumo.base.updated_at);
        let cid = insumo.base.company_id.to_string();
        let iid = insumo.base.id.to_string();
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        // 1. Metadados (mantém o estoque atual; o delta vem no passo 2).
        sqlx::query(
            "UPDATE insumos SET name = ?1, description = ?2, unit = ?3, stock_quantity = ?4, min_stock = ?5, cost_price = ?6, barcode = ?7, active = ?8, updated_at = ?9, synced = ?10
             WHERE company_id = ?11 AND id = ?12 AND deleted_at IS NULL",
        )
        .bind(&insumo.name)
        .bind(&insumo.description)
        .bind(&insumo.unit)
        .bind(insumo.stock_quantity)
        .bind(insumo.min_stock)
        .bind(insumo.cost_price.and_then(|d| d.to_f64()))
        .bind(&insumo.barcode)
        .bind(insumo.active)
        .bind(&updated)
        .bind(insumo.base.synced)
        .bind(&cid)
        .bind(&iid)
        .execute(&mut *tx)
        .await
        .map_err(map_db)?;

        // 2. Delta de estoque + ledger append-only, com guarda de não-negativo.
        if stock_delta.abs() > f64::EPSILON {
            let rows = sqlx::query(
                "UPDATE insumos SET stock_quantity = stock_quantity + ?1, updated_at = ?2, synced = 0
                  WHERE company_id = ?3 AND id = ?4 AND deleted_at IS NULL AND stock_quantity + ?1 >= 0",
            )
            .bind(stock_delta)
            .bind(&updated)
            .bind(&cid)
            .bind(&iid)
            .execute(&mut *tx)
            .await
            .map_err(map_db)?
            .rows_affected();
            if rows != 1 {
                tx.rollback().await.map_err(map_db)?;
                return Err(CoreError::Validation("Estoque insuficiente para o ajuste".into()));
            }
            insert_mv(&mut tx, insumo.base.company_id, insumo.base.id, stock_delta, "edit", None, &updated).await?;
        }
        tx.commit().await.map_err(map_db)?;
        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let now = ts(chrono::Utc::now().naive_utc());
        sqlx::query(
            "UPDATE insumos SET deleted_at = ?1, updated_at = ?2, synced = 0
             WHERE company_id = ?3 AND id = ?4 AND deleted_at IS NULL",
        )
        .bind(&now)
        .bind(&now)
        .bind(company_id.to_string())
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn try_adjust_stock(&self, company_id: Uuid, insumo_id: Uuid, delta: f64, reason: &str) -> Result<StockAdjustResult, CoreError> {
        let now = ts(chrono::Utc::now().naive_utc());
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        let result = sqlx::query(
            "UPDATE insumos SET stock_quantity = stock_quantity + ?1
              WHERE company_id = ?2 AND id = ?3 AND deleted_at IS NULL AND stock_quantity + ?1 >= 0",
        )
        .bind(delta)
        .bind(company_id.to_string())
        .bind(insumo_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(map_db)?;
        if result.rows_affected() == 1 {
            insert_mv(&mut tx, company_id, insumo_id, delta, reason, None, &now).await?;
            tx.commit().await.map_err(map_db)?;
            return Ok(StockAdjustResult::Adjusted);
        }
        tx.rollback().await.map_err(map_db)?;
        let row: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT deleted_at FROM insumos WHERE company_id = ?1 AND id = ?2",
        )
        .bind(company_id.to_string())
        .bind(insumo_id.to_string())
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
            "SELECT * FROM insumos WHERE company_id = ?1 AND synced = 0",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(Insumo::try_from).collect()
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query("UPDATE insumos SET synced = 1 WHERE company_id = ?1 AND id = ?2 AND updated_at = ?3")
            .bind(company_id.to_string())
            .bind(id.to_string())
            .bind(ts(updated_at))
            .execute(&self.pool)
            .await
            .map_err(map_db)?;
        Ok(())
    }

    async fn sync_upsert(&self, insumo: &Insumo) -> Result<(), CoreError> {
        // Estoque NÃO é sobrescrito no conflito quando há movimento pendente
        // (mesma guarda anti-overselling dos produtos): a quantidade evolui
        // pelo ledger; o LWW governa só os metadados.
        sqlx::query(
            "INSERT INTO insumos (id, company_id, name, description, unit, stock_quantity, min_stock, cost_price, barcode, active, created_at, updated_at, deleted_at, synced)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT (id) DO UPDATE SET
                 name = excluded.name,
                 description = excluded.description,
                 unit = excluded.unit,
                 stock_quantity = CASE
                     WHEN insumos.synced = 1
                          AND NOT EXISTS (SELECT 1 FROM insumo_movements m WHERE m.insumo_id = insumos.id AND m.synced = 0)
                     THEN excluded.stock_quantity
                     ELSE insumos.stock_quantity END,
                 min_stock = excluded.min_stock,
                 cost_price = excluded.cost_price,
                 barcode = excluded.barcode,
                 active = excluded.active,
                 updated_at = excluded.updated_at,
                 deleted_at = excluded.deleted_at,
                 synced = excluded.synced
             WHERE excluded.updated_at > insumos.updated_at",
        )
        .bind(insumo.base.id.to_string())
        .bind(insumo.base.company_id.to_string())
        .bind(&insumo.name)
        .bind(&insumo.description)
        .bind(&insumo.unit)
        .bind(insumo.stock_quantity)
        .bind(insumo.min_stock)
        .bind(insumo.cost_price.and_then(|d| d.to_f64()))
        .bind(&insumo.barcode)
        .bind(insumo.active)
        .bind(ts(insumo.base.created_at))
        .bind(ts(insumo.base.updated_at))
        .bind(insumo.base.deleted_at.map(ts))
        .bind(insumo.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn find_updated_since(&self, company_id: Uuid, since: chrono::NaiveDateTime) -> Result<Vec<Insumo>, CoreError> {
        let rows = sqlx::query_as::<_, InsumoRow>(
            "SELECT * FROM insumos WHERE company_id = ?1 AND updated_at > ?2 ORDER BY updated_at ASC",
        )
        .bind(company_id.to_string())
        .bind(ts(since))
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(Insumo::try_from).collect()
    }

    async fn find_unsynced_movements(&self, company_id: Uuid) -> Result<Vec<InsumoMovement>, CoreError> {
        let rows = sqlx::query_as::<_, InsumoMovementRow>(
            &format!("SELECT {MV_COLS} FROM insumo_movements WHERE company_id = ?1 AND synced = 0 ORDER BY created_at ASC"),
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(InsumoMovement::try_from).collect()
    }

    async fn mark_movement_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query("UPDATE insumo_movements SET synced = 1 WHERE company_id = ?1 AND id = ?2 AND updated_at = ?3")
            .bind(company_id.to_string())
            .bind(id.to_string())
            .bind(ts(updated_at))
            .execute(&self.pool)
            .await
            .map_err(map_db)?;
        Ok(())
    }

    async fn apply_movement(&self, m: &InsumoMovement) -> Result<(), CoreError> {
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        let inserted = sqlx::query(
            "INSERT OR IGNORE INTO insumo_movements (id, company_id, insumo_id, delta, reason, order_id, created_at, updated_at, deleted_at, synced)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1)",
        )
        .bind(m.base.id.to_string())
        .bind(m.base.company_id.to_string())
        .bind(m.insumo_id.to_string())
        .bind(m.delta)
        .bind(&m.reason)
        .bind(m.order_id.map(|o| o.to_string()))
        .bind(ts(m.base.created_at))
        .bind(ts(m.base.updated_at))
        .bind(m.base.deleted_at.map(ts))
        .execute(&mut *tx)
        .await
        .map_err(map_db)?
        .rows_affected();
        if inserted == 1 {
            sqlx::query("UPDATE insumos SET stock_quantity = stock_quantity + ?1 WHERE company_id = ?2 AND id = ?3")
                .bind(m.delta)
                .bind(m.base.company_id.to_string())
                .bind(m.insumo_id.to_string())
                .execute(&mut *tx)
                .await
                .map_err(map_db)?;
        }
        tx.commit().await.map_err(map_db)?;
        Ok(())
    }

    async fn insert_synced_movement(&self, m: &InsumoMovement) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT OR IGNORE INTO insumo_movements (id, company_id, insumo_id, delta, reason, order_id, created_at, updated_at, deleted_at, synced)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1)",
        )
        .bind(m.base.id.to_string())
        .bind(m.base.company_id.to_string())
        .bind(m.insumo_id.to_string())
        .bind(m.delta)
        .bind(&m.reason)
        .bind(m.order_id.map(|o| o.to_string()))
        .bind(ts(m.base.created_at))
        .bind(ts(m.base.updated_at))
        .bind(m.base.deleted_at.map(ts))
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn find_movements_updated_since(&self, company_id: Uuid, since: chrono::NaiveDateTime) -> Result<Vec<InsumoMovement>, CoreError> {
        let rows = sqlx::query_as::<_, InsumoMovementRow>(
            &format!("SELECT {MV_COLS} FROM insumo_movements WHERE company_id = ?1 AND updated_at > ?2 ORDER BY updated_at ASC"),
        )
        .bind(company_id.to_string())
        .bind(ts(since))
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(InsumoMovement::try_from).collect()
    }
}

/// Grava um movimento de insumo (ledger) dentro de uma transação aberta.
async fn insert_mv(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    company_id: Uuid,
    insumo_id: Uuid,
    delta: f64,
    reason: &str,
    order_id: Option<Uuid>,
    now: &str,
) -> Result<(), CoreError> {
    sqlx::query(
        "INSERT INTO insumo_movements (id, company_id, insumo_id, delta, reason, order_id, created_at, updated_at, deleted_at, synced)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, NULL, 0)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(company_id.to_string())
    .bind(insumo_id.to_string())
    .bind(delta)
    .bind(reason)
    .bind(order_id.map(|o| o.to_string()))
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(map_db)?;
    Ok(())
}
