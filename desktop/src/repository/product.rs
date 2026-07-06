use async_trait::async_trait;
use rust_decimal::prelude::ToPrimitive;
use sqlx::prelude::FromRow;
use sqlx::SqlitePool;
use uuid::Uuid;

use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;
use letaf_core::product::model::{BalanceMode, Product};
use letaf_core::product::repository::{ProductRepository, StockAdjustResult};
use letaf_core::product::stock_movement::StockMovement;

use super::helpers::{insert_stock_movement, map_db, parse_timestamp, parse_uuid, ts};

#[derive(FromRow)]
struct ProductRow {
    id: String,
    company_id: String,
    name: String,
    description: Option<String>,
    category_id: Option<String>,
    subcategory_id: Option<String>,
    price: Option<f64>,
    cost_price: Option<f64>,
    stock_quantity: f64,
    min_stock: f64,
    unlimited_stock: bool,
    barcode: Option<String>,
    unit: String,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    synced: bool,
    active: bool,
    web_visible: bool,
    balance_mode: String,
    image_data: Option<String>,
    cover_color: Option<String>,
    availability_schedule: Option<String>,
    discount_kind: Option<String>,
    discount_value: Option<f64>,
    discount_min_qty: Option<f64>,
    discount_tiers: Option<String>,
    variations: Option<String>,
}

impl TryFrom<ProductRow> for Product {
    type Error = CoreError;

    fn try_from(r: ProductRow) -> Result<Self, Self::Error> {
        let id = parse_uuid(&r.id)?;
        let balance_mode = BalanceMode::from_db_str(&r.balance_mode).unwrap_or_else(|| {
            tracing::warn!("BalanceMode desconhecido no banco: {:?} (product id={}); usando Weight", r.balance_mode, id);
            BalanceMode::Weight
        });
        Ok(Self {
            base: BaseFields {
                id,
                company_id: parse_uuid(&r.company_id)?,
                created_at: parse_timestamp(&r.created_at)?,
                updated_at: parse_timestamp(&r.updated_at)?,
                deleted_at: r.deleted_at.as_deref().map(parse_timestamp).transpose()?,
                synced: r.synced,
            },
            name: r.name,
            description: r.description,
            category_id: r.category_id.as_deref().map(parse_uuid).transpose()?,
            subcategory_id: r.subcategory_id.as_deref().map(parse_uuid).transpose()?,
            price: r.price.map(letaf_core::money::from_db_f64),
            cost_price: r.cost_price.map(letaf_core::money::from_db_f64),
            stock_quantity: r.stock_quantity,
            min_stock: r.min_stock,
            unlimited_stock: r.unlimited_stock,
            barcode: r.barcode,
            unit: r.unit,
            active: r.active,
            web_visible: r.web_visible,
            balance_mode,
            image_data: r.image_data,
            cover_color: r.cover_color,
            availability_schedule: r.availability_schedule,
            discount_kind: r.discount_kind,
            discount_value: r.discount_value.map(letaf_core::money::from_db_f64),
            discount_min_qty: r.discount_min_qty,
            discount_tiers: r.discount_tiers,
            addon_group_ids: Vec::new(),
            variations: r.variations,
        })
    }
}

#[derive(FromRow)]
struct StockMovementRow {
    id: String,
    company_id: String,
    product_id: String,
    delta: f64,
    reason: String,
    order_id: Option<String>,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    synced: bool,
}

impl TryFrom<StockMovementRow> for StockMovement {
    type Error = CoreError;
    fn try_from(r: StockMovementRow) -> Result<Self, Self::Error> {
        Ok(Self {
            base: BaseFields {
                id: parse_uuid(&r.id)?,
                company_id: parse_uuid(&r.company_id)?,
                created_at: parse_timestamp(&r.created_at)?,
                updated_at: parse_timestamp(&r.updated_at)?,
                deleted_at: r.deleted_at.as_deref().map(parse_timestamp).transpose()?,
                synced: r.synced,
            },
            product_id: parse_uuid(&r.product_id)?,
            delta: r.delta,
            reason: r.reason,
            order_id: r.order_id.as_deref().map(parse_uuid).transpose()?,
        })
    }
}

/// Implementação SQLite do ProductRepository.
///
/// Regras aplicadas (AI_RULES.md §3, §5, §7, §10):
/// - Desktop usa SQLite
/// - Todas queries filtram por company_id (isolamento)
/// - Soft delete via deleted_at
/// - Acesso ao banco somente via repository
/// - Offline-first: toda escrita ocorre primeiro no SQLite
pub struct SqliteProductRepository {
    pool: SqlitePool,
}

impl SqliteProductRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Carrega `addon_group_ids` para uma lista de produtos em 1 query.
    /// SQLite não tem `= ANY($1)` nativo — usamos uma única query sem
    /// filtro de IDs e batemos em memória (lista pequena por empresa).
    async fn hydrate_addon_group_ids(&self, company_id: Uuid, products: &mut [Product]) -> Result<(), CoreError> {
        if products.is_empty() { return Ok(()); }
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT product_id, group_id FROM product_addon_groups
             WHERE company_id = ?1
             ORDER BY sort_order ASC",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        let mut parsed: Vec<(Uuid, Uuid)> = Vec::with_capacity(rows.len());
        for (pid, gid) in rows {
            parsed.push((parse_uuid(&pid)?, parse_uuid(&gid)?));
        }
        for p in products.iter_mut() {
            p.addon_group_ids = parsed.iter()
                .filter(|(pid, _)| *pid == p.base.id)
                .map(|(_, gid)| *gid)
                .collect();
        }
        Ok(())
    }
}

#[async_trait]
impl ProductRepository for SqliteProductRepository {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Product>, CoreError> {
        let row = sqlx::query_as::<_, ProductRow>(
            "SELECT * FROM products WHERE company_id = ?1 AND id = ?2 AND deleted_at IS NULL",
        )
        .bind(company_id.to_string())
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;

        let Some(row) = row else { return Ok(None); };
        let mut product = Product::try_from(row)?;
        product.addon_group_ids = self.find_addon_group_ids(company_id, product.base.id).await?;
        Ok(Some(product))
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<Product>, CoreError> {
        let rows = sqlx::query_as::<_, ProductRow>(
            "SELECT * FROM products WHERE company_id = ?1 AND deleted_at IS NULL ORDER BY created_at DESC",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        let mut products: Vec<Product> = rows.into_iter()
            .map(Product::try_from)
            .collect::<Result<_, _>>()?;
        self.hydrate_addon_group_ids(company_id, &mut products).await?;
        Ok(products)
    }

    async fn find_by_ids(&self, company_id: Uuid, ids: &[Uuid]) -> Result<Vec<Product>, CoreError> {
        if ids.is_empty() { return Ok(Vec::new()); }
        // SQLite não tem `ANY`; monta `IN (?2, ?3, ...)` com placeholders
        // (sem interpolar valores — sem risco de injeção).
        let placeholders = (0..ids.len())
            .map(|i| format!("?{}", i + 2))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT * FROM products WHERE company_id = ?1 AND id IN ({placeholders}) AND deleted_at IS NULL"
        );
        let mut q = sqlx::query_as::<_, ProductRow>(&sql).bind(company_id.to_string());
        for id in ids {
            q = q.bind(id.to_string());
        }
        let rows = q.fetch_all(&self.pool).await.map_err(map_db)?;
        let mut products: Vec<Product> = rows.into_iter()
            .map(Product::try_from)
            .collect::<Result<_, _>>()?;
        self.hydrate_addon_group_ids(company_id, &mut products).await?;
        Ok(products)
    }

    async fn create(&self, product: &Product) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO products (id, company_id, name, description, category_id, subcategory_id, price, cost_price, stock_quantity, unlimited_stock, barcode, unit, created_at, updated_at, deleted_at, synced, active, web_visible, balance_mode, image_data, cover_color, availability_schedule, discount_kind, discount_value, discount_min_qty, discount_tiers, variations, min_stock)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28)"
        )
        .bind(product.base.id.to_string())
        .bind(product.base.company_id.to_string())
        .bind(&product.name)
        .bind(&product.description)
        .bind(product.category_id.map(|u| u.to_string()))
        .bind(product.subcategory_id.map(|u| u.to_string()))
        .bind(product.price.and_then(|d| d.to_f64()))
        .bind(product.cost_price.and_then(|d| d.to_f64()))
        .bind(product.stock_quantity)
        .bind(product.unlimited_stock)
        .bind(&product.barcode)
        .bind(&product.unit)
        .bind(ts(product.base.created_at))
        .bind(ts(product.base.updated_at))
        .bind(product.base.deleted_at.map(ts))
        .bind(product.base.synced)
        .bind(product.active)
        .bind(product.web_visible)
        .bind(product.balance_mode.as_db_str())
        .bind(&product.image_data)
        .bind(&product.cover_color)
        .bind(&product.availability_schedule)
        .bind(&product.discount_kind)
        .bind(product.discount_value.and_then(|d| d.to_f64()))
        .bind(product.discount_min_qty)
        .bind(&product.discount_tiers)
        .bind(&product.variations)
        .bind(product.min_stock)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    async fn update(&self, product: &Product) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE products SET name = ?1, description = ?2, category_id = ?3, subcategory_id = ?4, price = ?5, cost_price = ?6, stock_quantity = ?7, unlimited_stock = ?8, barcode = ?9, unit = ?10, balance_mode = ?11, updated_at = ?12, synced = ?13, image_data = ?14, cover_color = ?15, availability_schedule = ?16, discount_kind = ?17, discount_value = ?18, discount_min_qty = ?19, discount_tiers = ?20, variations = ?21, min_stock = ?22
             WHERE company_id = ?23 AND id = ?24 AND deleted_at IS NULL",
            // Nota: active e web_visible são alterados via toggle_* (AI_RULES.md §8)
        )
        .bind(&product.name)
        .bind(&product.description)
        .bind(product.category_id.map(|u| u.to_string()))
        .bind(product.subcategory_id.map(|u| u.to_string()))
        .bind(product.price.and_then(|d| d.to_f64()))
        .bind(product.cost_price.and_then(|d| d.to_f64()))
        .bind(product.stock_quantity)
        .bind(product.unlimited_stock)
        .bind(&product.barcode)
        .bind(&product.unit)
        .bind(product.balance_mode.as_db_str())
        .bind(ts(product.base.updated_at))
        .bind(product.base.synced)
        .bind(&product.image_data)
        .bind(&product.cover_color)
        .bind(&product.availability_schedule)
        .bind(&product.discount_kind)
        .bind(product.discount_value.and_then(|d| d.to_f64()))
        .bind(product.discount_min_qty)
        .bind(&product.discount_tiers)
        .bind(&product.variations)
        .bind(product.min_stock)
        .bind(product.base.company_id.to_string())
        .bind(product.base.id.to_string())
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let now = ts(chrono::Utc::now().naive_utc());
        sqlx::query(
            "UPDATE products SET deleted_at = ?1, updated_at = ?2, synced = false
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

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Product>, CoreError> {
        let rows = sqlx::query_as::<_, ProductRow>(
            "SELECT * FROM products WHERE company_id = ?1 AND synced = false",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        let mut products: Vec<Product> = rows.into_iter()
            .map(Product::try_from)
            .collect::<Result<_, _>>()?;
        self.hydrate_addon_group_ids(company_id, &mut products).await?;
        Ok(products)
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query("UPDATE products SET synced = true WHERE company_id = ?1 AND id = ?2 AND updated_at = ?3")
            .bind(company_id.to_string())
            .bind(id.to_string())
            .bind(ts(updated_at))
            .execute(&self.pool)
            .await
            .map_err(map_db)?;

        Ok(())
    }

    async fn find_active(&self, company_id: Uuid) -> Result<Vec<Product>, CoreError> {
        let rows = sqlx::query_as::<_, ProductRow>(
            "SELECT * FROM products WHERE company_id = ?1 AND deleted_at IS NULL AND active = true AND web_visible = true ORDER BY created_at DESC",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        let mut products: Vec<Product> = rows.into_iter()
            .map(Product::try_from)
            .collect::<Result<_, _>>()?;
        self.hydrate_addon_group_ids(company_id, &mut products).await?;
        Ok(products)
    }

    async fn toggle_active(&self, company_id: Uuid, id: Uuid, active: bool) -> Result<(), CoreError> {
        let now = ts(chrono::Utc::now().naive_utc());
        sqlx::query(
            "UPDATE products SET active = ?1, updated_at = ?2, synced = false
             WHERE company_id = ?3 AND id = ?4 AND deleted_at IS NULL",
        )
        .bind(active)
        .bind(&now)
        .bind(company_id.to_string())
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    async fn toggle_web_visible(&self, company_id: Uuid, id: Uuid, visible: bool) -> Result<(), CoreError> {
        let now = ts(chrono::Utc::now().naive_utc());
        sqlx::query(
            "UPDATE products SET web_visible = ?1, updated_at = ?2, synced = false
             WHERE company_id = ?3 AND id = ?4 AND deleted_at IS NULL",
        )
        .bind(visible)
        .bind(&now)
        .bind(company_id.to_string())
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    async fn find_updated_since(&self, company_id: Uuid, since: chrono::NaiveDateTime) -> Result<Vec<Product>, CoreError> {
        let rows = sqlx::query_as::<_, ProductRow>(
            "SELECT * FROM products WHERE company_id = ?1 AND updated_at > ?2",
        )
        .bind(company_id.to_string())
        .bind(ts(since))
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        let mut products: Vec<Product> = rows.into_iter()
            .map(Product::try_from)
            .collect::<Result<_, _>>()?;
        self.hydrate_addon_group_ids(company_id, &mut products).await?;
        Ok(products)
    }

    async fn find_addon_group_ids(&self, company_id: Uuid, product_id: Uuid) -> Result<Vec<Uuid>, CoreError> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT group_id FROM product_addon_groups
             WHERE company_id = ?1 AND product_id = ?2
             ORDER BY sort_order ASC",
        )
        .bind(company_id.to_string())
        .bind(product_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        let mut ids = Vec::with_capacity(rows.len());
        for (g,) in rows { ids.push(parse_uuid(&g)?); }
        Ok(ids)
    }

    /// Reescreve a lista de associações em transação SQLite.
    async fn replace_addon_groups(
        &self,
        company_id: Uuid,
        product_id: Uuid,
        group_ids: &[Uuid],
    ) -> Result<(), CoreError> {
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        sqlx::query(
            "DELETE FROM product_addon_groups WHERE company_id = ?1 AND product_id = ?2",
        )
        .bind(company_id.to_string())
        .bind(product_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(map_db)?;
        for (idx, gid) in group_ids.iter().enumerate() {
            sqlx::query(
                "INSERT INTO product_addon_groups (company_id, product_id, group_id, sort_order)
                 VALUES (?1, ?2, ?3, ?4)",
            )
            .bind(company_id.to_string())
            .bind(product_id.to_string())
            .bind(gid.to_string())
            .bind(idx as i32)
            .execute(&mut *tx)
            .await
            .map_err(map_db)?;
        }
        tx.commit().await.map_err(map_db)?;
        Ok(())
    }

    /// UPDATE atômico (SQLite). `bool` em SQLite é armazenado como
    /// integer (0/1); a condição `unlimited_stock = false` mapeia para
    /// `= 0`.
    async fn try_adjust_stock(
        &self,
        company_id: Uuid,
        product_id: Uuid,
        delta: f64,
    ) -> Result<StockAdjustResult, CoreError> {
        let now = ts(chrono::Utc::now().naive_utc());
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        let result = sqlx::query(
            "UPDATE products
                SET stock_quantity = stock_quantity + ?1,
                    updated_at = ?2,
                    synced = 0
              WHERE company_id = ?3
                AND id = ?4
                AND deleted_at IS NULL
                AND unlimited_stock = 0
                AND stock_quantity + ?1 >= 0",
        )
        .bind(delta)
        .bind(&now)
        .bind(company_id.to_string())
        .bind(product_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(map_db)?;
        if result.rows_affected() == 1 {
            // Ledger append-only: registra o delta na MESMA transação (§7),
            // base do sync idempotente que substitui o LWW sobre o absoluto.
            insert_stock_movement(&mut tx, company_id, product_id, delta, "adjust", None, &now)
                .await?;
            tx.commit().await.map_err(map_db)?;
            return Ok(StockAdjustResult::Adjusted);
        }
        tx.rollback().await.map_err(map_db)?;
        let row: Option<(bool, f64, Option<String>)> = sqlx::query_as(
            "SELECT unlimited_stock, stock_quantity, deleted_at FROM products
              WHERE company_id = ?1 AND id = ?2",
        )
        .bind(company_id.to_string())
        .bind(product_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        match row {
            None => Ok(StockAdjustResult::NotFound),
            Some((_, _, Some(_))) => Ok(StockAdjustResult::NotFound),
            Some((true, _, _)) => Ok(StockAdjustResult::Unlimited),
            Some((false, _, _)) => Ok(StockAdjustResult::Insufficient),
        }
    }

    async fn sync_upsert(&self, product: &Product) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO products (id, company_id, name, description, category_id, subcategory_id, price, cost_price, stock_quantity, unlimited_stock, barcode, unit, created_at, updated_at, deleted_at, synced, active, web_visible, balance_mode, image_data, cover_color, availability_schedule, discount_kind, discount_value, discount_min_qty, discount_tiers, variations, min_stock)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28)
             ON CONFLICT (id) DO UPDATE SET
                 name = excluded.name,
                 description = excluded.description,
                 category_id = excluded.category_id,
                 subcategory_id = excluded.subcategory_id,
                 price = excluded.price,
                 cost_price = excluded.cost_price,
                 -- Estoque: só aceita o absoluto do servidor quando o registro
                 -- local está sincronizado (synced=1). Se há venda/ajuste local
                 -- pendente (synced=0), preserva o valor local — o servidor
                 -- converge depois que o movimento pendente for empurrado e
                 -- aplicado lá. Evita o overselling do LWW sobre o absoluto (§7).
                 stock_quantity = CASE WHEN products.synced = 1
                                       THEN excluded.stock_quantity
                                       ELSE products.stock_quantity END,
                 unlimited_stock = excluded.unlimited_stock,
                 barcode = excluded.barcode,
                 unit = excluded.unit,
                 updated_at = excluded.updated_at,
                 deleted_at = excluded.deleted_at,
                 synced = excluded.synced,
                 active = excluded.active,
                 web_visible = excluded.web_visible,
                 balance_mode = excluded.balance_mode,
                 image_data = excluded.image_data,
                 cover_color = excluded.cover_color,
                 availability_schedule = excluded.availability_schedule,
                 discount_kind = excluded.discount_kind,
                 discount_value = excluded.discount_value,
                 discount_min_qty = excluded.discount_min_qty,
                 discount_tiers = excluded.discount_tiers,
                 variations = excluded.variations,
                 min_stock = excluded.min_stock
             WHERE excluded.updated_at > products.updated_at",
        )
        .bind(product.base.id.to_string())
        .bind(product.base.company_id.to_string())
        .bind(&product.name)
        .bind(&product.description)
        .bind(product.category_id.map(|u| u.to_string()))
        .bind(product.subcategory_id.map(|u| u.to_string()))
        .bind(product.price.and_then(|d| d.to_f64()))
        .bind(product.cost_price.and_then(|d| d.to_f64()))
        .bind(product.stock_quantity)
        .bind(product.unlimited_stock)
        .bind(&product.barcode)
        .bind(&product.unit)
        .bind(ts(product.base.created_at))
        .bind(ts(product.base.updated_at))
        .bind(product.base.deleted_at.map(ts))
        .bind(product.base.synced)
        .bind(product.active)
        .bind(product.web_visible)
        .bind(product.balance_mode.as_db_str())
        .bind(&product.image_data)
        .bind(&product.cover_color)
        .bind(&product.availability_schedule)
        .bind(&product.discount_kind)
        .bind(product.discount_value.and_then(|d| d.to_f64()))
        .bind(product.discount_min_qty)
        .bind(&product.discount_tiers)
        .bind(&product.variations)
        .bind(product.min_stock)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    async fn find_unsynced_stock_movements(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<StockMovement>, CoreError> {
        let rows = sqlx::query_as::<_, StockMovementRow>(
            "SELECT id, company_id, product_id, delta, reason, order_id, created_at, updated_at, deleted_at, synced
             FROM stock_movements WHERE company_id = ?1 AND synced = 0 ORDER BY created_at ASC",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(StockMovement::try_from).collect()
    }

    async fn mark_stock_movement_synced(
        &self,
        company_id: Uuid,
        id: Uuid,
        updated_at: chrono::NaiveDateTime,
    ) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE stock_movements SET synced = 1
             WHERE company_id = ?1 AND id = ?2 AND updated_at = ?3",
        )
        .bind(company_id.to_string())
        .bind(id.to_string())
        .bind(ts(updated_at))
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn apply_stock_movement(&self, m: &StockMovement) -> Result<(), CoreError> {
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        // `INSERT OR IGNORE` garante idempotência: se o id já existe, não
        // reinsere e o delta NÃO é reaplicado. Movimento aplicado = synced.
        let inserted = sqlx::query(
            "INSERT OR IGNORE INTO stock_movements
                (id, company_id, product_id, delta, reason, order_id, created_at, updated_at, deleted_at, synced)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1)",
        )
        .bind(m.base.id.to_string())
        .bind(m.base.company_id.to_string())
        .bind(m.product_id.to_string())
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
            // Aplica o delta ao materializado só na 1ª vez. Não toca
            // updated_at/synced do produto (a mudança veio do sync, não
            // deve reempurrar o produto). Ilimitado não decrementa.
            sqlx::query(
                "UPDATE products SET stock_quantity = stock_quantity + ?1
                 WHERE company_id = ?2 AND id = ?3 AND unlimited_stock = 0",
            )
            .bind(m.delta)
            .bind(m.base.company_id.to_string())
            .bind(m.product_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(map_db)?;
        }
        tx.commit().await.map_err(map_db)?;
        Ok(())
    }

    async fn find_stock_movements_updated_since(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
    ) -> Result<Vec<StockMovement>, CoreError> {
        let rows = sqlx::query_as::<_, StockMovementRow>(
            "SELECT id, company_id, product_id, delta, reason, order_id, created_at, updated_at, deleted_at, synced
             FROM stock_movements WHERE company_id = ?1 AND updated_at > ?2 ORDER BY updated_at ASC",
        )
        .bind(company_id.to_string())
        .bind(ts(since))
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(StockMovement::try_from).collect()
    }
}
