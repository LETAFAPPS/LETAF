use async_trait::async_trait;
use rust_decimal::Decimal;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::PgPool;
use uuid::Uuid;

use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;
use letaf_core::product::model::{BalanceMode, Product};
use letaf_core::product::repository::{ProductRepository, StockAdjustResult};
use letaf_core::product::stock_movement::StockMovement;

use super::helpers::{insert_stock_movement, keyset_pull_sql, map_db};

/// Row intermediário sqlx → StockMovement (tipos nativos do Postgres).
#[derive(FromRow)]
struct StockMovementRow {
    id: Uuid,
    company_id: Uuid,
    product_id: Uuid,
    delta: f64,
    reason: String,
    order_id: Option<Uuid>,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    deleted_at: Option<NaiveDateTime>,
    synced: bool,
}

impl From<StockMovementRow> for StockMovement {
    fn from(r: StockMovementRow) -> Self {
        Self {
            base: BaseFields {
                id: r.id,
                company_id: r.company_id,
                created_at: r.created_at,
                updated_at: r.updated_at,
                deleted_at: r.deleted_at,
                synced: r.synced,
            },
            product_id: r.product_id,
            delta: r.delta,
            reason: r.reason,
            order_id: r.order_id,
        }
    }
}

/// Row intermediário para mapeamento sqlx → domínio.
///
/// Regras aplicadas (AI_RULES.md §1, §10):
/// - Core não depende de sqlx
/// - Row struct vive na camada server (infraestrutura)
#[derive(FromRow)]
struct ProductRow {
    id: Uuid,
    company_id: Uuid,
    name: String,
    description: Option<String>,
    category_id: Option<Uuid>,
    subcategory_id: Option<Uuid>,
    price: Option<Decimal>,
    cost_price: Option<Decimal>,
    stock_quantity: f64,
    min_stock: f64,
    unlimited_stock: bool,
    barcode: Option<String>,
    unit: String,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    deleted_at: Option<NaiveDateTime>,
    synced: bool,
    active: bool,
    web_visible: bool,
    balance_mode: String,
    image_data: Option<String>,
    cover_color: Option<String>,
    availability_schedule: Option<String>,
    discount_kind: Option<String>,
    discount_value: Option<Decimal>,
    discount_min_qty: Option<f64>,
    discount_tiers: Option<String>,
    variations: Option<String>,
}

impl From<ProductRow> for Product {
    fn from(r: ProductRow) -> Self {
        let balance_mode = BalanceMode::from_db_str(&r.balance_mode).unwrap_or_else(|| {
            tracing::warn!("BalanceMode desconhecido no banco: {:?} (product id={}); usando Weight", r.balance_mode, r.id);
            BalanceMode::Weight
        });
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
            category_id: r.category_id,
            subcategory_id: r.subcategory_id,
            price: r.price,
            cost_price: r.cost_price,
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
            discount_value: r.discount_value,
            discount_min_qty: r.discount_min_qty,
            discount_tiers: r.discount_tiers,
            // Carregado sob demanda pelo repo via `find_addon_group_ids`
            // — não vem na linha de products.
            addon_group_ids: Vec::new(),
            variations: r.variations,
        }
    }
}

/// Implementação PostgreSQL do ProductRepository.
///
/// Regras aplicadas (AI_RULES.md §3, §5, §6, §10):
/// - Todas queries filtram por company_id (isolamento)
/// - Soft delete via deleted_at
/// - Servidor usa PostgreSQL
/// - Acesso ao banco somente via repository
pub struct PgProductRepository {
    pool: PgPool,
}

impl PgProductRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl PgProductRepository {
    /// Carrega `addon_group_ids` em batch para uma lista de produtos.
    /// Single query JOIN evita N+1 quando o catálogo lista muitos itens.
    async fn hydrate_addon_group_ids(&self, company_id: Uuid, products: &mut [Product]) -> Result<(), CoreError> {
        if products.is_empty() { return Ok(()); }
        let ids: Vec<Uuid> = products.iter().map(|p| p.base.id).collect();
        let rows: Vec<(Uuid, Uuid)> = sqlx::query_as(
            "SELECT product_id, group_id FROM product_addon_groups
             WHERE company_id = $1 AND product_id = ANY($2)
             ORDER BY sort_order ASC",
        )
        .bind(company_id)
        .bind(&ids)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        // Agrupa uma vez (O(R)) em vez de re-escanear `rows` por produto
        // (O(P×R)). `ORDER BY sort_order` preservado pelo push em ordem.
        let mut by_product: std::collections::HashMap<Uuid, Vec<Uuid>> =
            std::collections::HashMap::new();
        for (pid, gid) in rows {
            by_product.entry(pid).or_default().push(gid);
        }
        for p in products.iter_mut() {
            p.addon_group_ids = by_product.remove(&p.base.id).unwrap_or_default();
        }
        Ok(())
    }
}

#[async_trait]
impl ProductRepository for PgProductRepository {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Product>, CoreError> {
        let row = sqlx::query_as::<_, ProductRow>(
            "SELECT * FROM products WHERE company_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(company_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        let Some(row) = row else { return Ok(None); };
        let mut product = Product::from(row);
        product.addon_group_ids = self.find_addon_group_ids(company_id, product.base.id).await?;
        Ok(Some(product))
    }

    /// Override eficiente: `COUNT(*)` em vez de materializar as linhas (§13).
    async fn count_all(&self, company_id: Uuid) -> Result<i64, CoreError> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM products WHERE company_id = $1 AND deleted_at IS NULL",
        )
        .bind(company_id)
        .fetch_one(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(row.0)
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<Product>, CoreError> {
        let rows = sqlx::query_as::<_, ProductRow>(
            "SELECT * FROM products WHERE company_id = $1 AND deleted_at IS NULL ORDER BY created_at DESC",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        let mut products: Vec<Product> = rows.into_iter().map(Product::from).collect();
        self.hydrate_addon_group_ids(company_id, &mut products).await?;
        Ok(products)
    }

    async fn find_by_ids(&self, company_id: Uuid, ids: &[Uuid]) -> Result<Vec<Product>, CoreError> {
        if ids.is_empty() { return Ok(Vec::new()); }
        let rows = sqlx::query_as::<_, ProductRow>(
            "SELECT * FROM products WHERE company_id = $1 AND id = ANY($2) AND deleted_at IS NULL",
        )
        .bind(company_id)
        .bind(ids)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        let mut products: Vec<Product> = rows.into_iter().map(Product::from).collect();
        self.hydrate_addon_group_ids(company_id, &mut products).await?;
        Ok(products)
    }

    async fn create(&self, product: &Product) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO products (id, company_id, name, description, category_id, subcategory_id, price, cost_price, stock_quantity, unlimited_stock, barcode, unit, created_at, updated_at, deleted_at, synced, active, web_visible, balance_mode, image_data, cover_color, availability_schedule, discount_kind, discount_value, discount_min_qty, discount_tiers, variations, min_stock)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28)",
        )
        .bind(product.base.id)
        .bind(product.base.company_id)
        .bind(&product.name)
        .bind(&product.description)
        .bind(product.category_id)
        .bind(product.subcategory_id)
        .bind(product.price)
        .bind(product.cost_price)
        .bind(product.stock_quantity)
        .bind(product.unlimited_stock)
        .bind(&product.barcode)
        .bind(&product.unit)
        .bind(product.base.created_at)
        .bind(product.base.updated_at)
        .bind(product.base.deleted_at)
        .bind(product.base.synced)
        .bind(product.active)
        .bind(product.web_visible)
        .bind(product.balance_mode.as_db_str())
        .bind(&product.image_data)
        .bind(&product.cover_color)
        .bind(&product.availability_schedule)
        .bind(&product.discount_kind)
        .bind(product.discount_value)
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
            "UPDATE products SET name = $1, description = $2, category_id = $3, subcategory_id = $4, price = $5, cost_price = $6, stock_quantity = $7, unlimited_stock = $8, barcode = $9, unit = $10, balance_mode = $11, image_data = $12, cover_color = $13, availability_schedule = $14, discount_kind = $15, discount_value = $16, discount_min_qty = $17, discount_tiers = $18, variations = $19, min_stock = $20, updated_at = $21, synced = $22
             WHERE company_id = $23 AND id = $24 AND deleted_at IS NULL",
        )
        .bind(&product.name)
        .bind(&product.description)
        .bind(product.category_id)
        .bind(product.subcategory_id)
        .bind(product.price)
        .bind(product.cost_price)
        .bind(product.stock_quantity)
        .bind(product.unlimited_stock)
        .bind(&product.barcode)
        .bind(&product.unit)
        .bind(product.balance_mode.as_db_str())
        .bind(&product.image_data)
        .bind(&product.cover_color)
        .bind(&product.availability_schedule)
        .bind(&product.discount_kind)
        .bind(product.discount_value)
        .bind(product.discount_min_qty)
        .bind(&product.discount_tiers)
        .bind(&product.variations)
        .bind(product.min_stock)
        .bind(product.base.updated_at)
        .bind(product.base.synced)
        .bind(product.base.company_id)
        .bind(product.base.id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    async fn update_atomic(
        &self,
        product: &Product,
        stock_delta: f64,
        group_ids: &[Uuid],
    ) -> Result<(), CoreError> {
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        // 1. Metadados (mantém `stock_quantity` atual; o delta vem no passo 2).
        sqlx::query(
            "UPDATE products SET name = $1, description = $2, category_id = $3, subcategory_id = $4, price = $5, cost_price = $6, stock_quantity = $7, unlimited_stock = $8, barcode = $9, unit = $10, balance_mode = $11, image_data = $12, cover_color = $13, availability_schedule = $14, discount_kind = $15, discount_value = $16, discount_min_qty = $17, discount_tiers = $18, variations = $19, min_stock = $20, updated_at = $21, synced = $22
             WHERE company_id = $23 AND id = $24 AND deleted_at IS NULL",
        )
        .bind(&product.name)
        .bind(&product.description)
        .bind(product.category_id)
        .bind(product.subcategory_id)
        .bind(product.price)
        .bind(product.cost_price)
        .bind(product.stock_quantity)
        .bind(product.unlimited_stock)
        .bind(&product.barcode)
        .bind(&product.unit)
        .bind(product.balance_mode.as_db_str())
        .bind(&product.image_data)
        .bind(&product.cover_color)
        .bind(&product.availability_schedule)
        .bind(&product.discount_kind)
        .bind(product.discount_value)
        .bind(product.discount_min_qty)
        .bind(&product.discount_tiers)
        .bind(&product.variations)
        .bind(product.min_stock)
        .bind(product.base.updated_at)
        .bind(product.base.synced)
        .bind(product.base.company_id)
        .bind(product.base.id)
        .execute(&mut *tx)
        .await
        .map_err(map_db)?;

        // 2. Delta de estoque + ledger append-only (§7), com guarda de não-negativo.
        if stock_delta.abs() > f64::EPSILON && !product.unlimited_stock {
            let rows = sqlx::query(
                "UPDATE products
                    SET stock_quantity = stock_quantity + $1, updated_at = $2, synced = false
                  WHERE company_id = $3 AND id = $4 AND deleted_at IS NULL
                    AND unlimited_stock = false AND stock_quantity + $1 >= 0",
            )
            .bind(stock_delta)
            .bind(product.base.updated_at)
            .bind(product.base.company_id)
            .bind(product.base.id)
            .execute(&mut *tx)
            .await
            .map_err(map_db)?
            .rows_affected();
            if rows != 1 {
                tx.rollback().await.map_err(map_db)?;
                return Err(CoreError::Validation(
                    "Estoque insuficiente para o ajuste".into(),
                ));
            }
            insert_stock_movement(
                &mut tx,
                product.base.company_id,
                product.base.id,
                stock_delta,
                "edit",
                None,
                product.base.updated_at,
            )
            .await?;
        }

        // 3. Reescreve as associações N:M de adicionais (DELETE + INSERT).
        sqlx::query("DELETE FROM product_addon_groups WHERE company_id = $1 AND product_id = $2")
            .bind(product.base.company_id)
            .bind(product.base.id)
            .execute(&mut *tx)
            .await
            .map_err(map_db)?;
        for (idx, gid) in group_ids.iter().enumerate() {
            sqlx::query(
                "INSERT INTO product_addon_groups (company_id, product_id, group_id, sort_order)
                 VALUES ($1, $2, $3, $4)",
            )
            .bind(product.base.company_id)
            .bind(product.base.id)
            .bind(gid)
            .bind(idx as i32)
            .execute(&mut *tx)
            .await
            .map_err(map_db)?;
        }

        tx.commit().await.map_err(map_db)?;
        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        sqlx::query(
            "UPDATE products SET deleted_at = $1, updated_at = $2, synced = false
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

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Product>, CoreError> {
        let rows = sqlx::query_as::<_, ProductRow>(
            "SELECT * FROM products WHERE company_id = $1 AND synced = false",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        let mut products: Vec<Product> = rows.into_iter().map(Product::from).collect();
        self.hydrate_addon_group_ids(company_id, &mut products).await?;
        Ok(products)
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE products SET synced = true WHERE company_id = $1 AND id = $2 AND updated_at = $3",
        )
        .bind(company_id)
        .bind(id)
        .bind(updated_at)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    /// Catálogo público: ativos E visíveis na web.
    ///
    /// §13: NÃO puxa o blob `image_data` (base64, potencialmente centenas de KB
    /// por produto) — a rota pública mais quente só checa se HÁ imagem para
    /// montar a URL de `/catalog/media/product/{id}` (a imagem em si é servida
    /// por esse endpoint separado). Retorna um sentinela de 1 byte no lugar do
    /// blob (`'1'`/NULL); o resto das colunas é explícito.
    async fn find_active(&self, company_id: Uuid) -> Result<Vec<Product>, CoreError> {
        let rows = sqlx::query_as::<_, ProductRow>(
            "SELECT id, company_id, name, description, category_id, subcategory_id, price, cost_price,
                    stock_quantity, unlimited_stock, barcode, unit, created_at, updated_at, deleted_at,
                    synced, active, web_visible, balance_mode,
                    CASE WHEN image_data IS NOT NULL THEN '1' END AS image_data,
                    cover_color, availability_schedule, discount_kind, discount_value, discount_min_qty,
                    discount_tiers, variations, min_stock
               FROM products
              WHERE company_id = $1 AND deleted_at IS NULL AND active = true AND web_visible = true
              ORDER BY created_at DESC",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        let mut products: Vec<Product> = rows.into_iter().map(Product::from).collect();
        self.hydrate_addon_group_ids(company_id, &mut products).await?;
        Ok(products)
    }

    async fn find_image_data(&self, company_id: Uuid, id: Uuid) -> Result<Option<String>, CoreError> {
        let row: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT image_data FROM products WHERE company_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(company_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(row.and_then(|(img,)| img))
    }

    async fn toggle_active(&self, company_id: Uuid, id: Uuid, active: bool) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        sqlx::query(
            "UPDATE products SET active = $1, updated_at = $2, synced = false
             WHERE company_id = $3 AND id = $4 AND deleted_at IS NULL",
        )
        .bind(active)
        .bind(now)
        .bind(company_id)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    async fn toggle_web_visible(&self, company_id: Uuid, id: Uuid, visible: bool) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        sqlx::query(
            "UPDATE products SET web_visible = $1, updated_at = $2, synced = false
             WHERE company_id = $3 AND id = $4 AND deleted_at IS NULL",
        )
        .bind(visible)
        .bind(now)
        .bind(company_id)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    async fn find_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<Product>, CoreError> {
        let rows = sqlx::query_as::<_, ProductRow>(
            "SELECT * FROM products WHERE company_id = $1 AND updated_at > $2",
        )
        .bind(company_id)
        .bind(since)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        let mut products: Vec<Product> = rows.into_iter().map(Product::from).collect();
        self.hydrate_addon_group_ids(company_id, &mut products).await?;
        Ok(products)
    }

    async fn find_updated_since_paged(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
        after_id: Uuid,
        limit: i64,
    ) -> Result<Vec<Product>, CoreError> {
        let rows = sqlx::query_as::<_, ProductRow>(&keyset_pull_sql("products"))
        .bind(company_id)
        .bind(since)
        .bind(after_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        let mut products: Vec<Product> = rows.into_iter().map(Product::from).collect();
        self.hydrate_addon_group_ids(company_id, &mut products).await?;
        Ok(products)
    }

    async fn find_addon_group_ids(&self, company_id: Uuid, product_id: Uuid) -> Result<Vec<Uuid>, CoreError> {
        let rows: Vec<(Uuid,)> = sqlx::query_as(
            "SELECT group_id FROM product_addon_groups
             WHERE company_id = $1 AND product_id = $2
             ORDER BY sort_order ASC",
        )
        .bind(company_id)
        .bind(product_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(|(g,)| g).collect())
    }

    /// Reescreve a lista de associações: deleta tudo e reinsere com a
    /// nova ordem. Operação em transação para evitar estado parcial em
    /// caso de erro.
    async fn replace_addon_groups(
        &self,
        company_id: Uuid,
        product_id: Uuid,
        group_ids: &[Uuid],
    ) -> Result<(), CoreError> {
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        sqlx::query(
            "DELETE FROM product_addon_groups WHERE company_id = $1 AND product_id = $2",
        )
        .bind(company_id)
        .bind(product_id)
        .execute(&mut *tx)
        .await
        .map_err(map_db)?;
        for (idx, gid) in group_ids.iter().enumerate() {
            sqlx::query(
                "INSERT INTO product_addon_groups (company_id, product_id, group_id, sort_order)
                 VALUES ($1, $2, $3, $4)",
            )
            .bind(company_id)
            .bind(product_id)
            .bind(gid)
            .bind(idx as i32)
            .execute(&mut *tx)
            .await
            .map_err(map_db)?;
        }
        tx.commit().await.map_err(map_db)?;
        Ok(())
    }

    /// UPDATE atômico: ajusta o estoque numa única query, eliminando
    /// race entre `find` e `update` quando dois clientes vendem o
    /// mesmo produto em paralelo. Não toca produtos `unlimited_stock`.
    async fn try_adjust_stock(
        &self,
        company_id: Uuid,
        product_id: Uuid,
        delta: f64,
    ) -> Result<StockAdjustResult, CoreError> {
        let now = chrono::Utc::now().naive_utc();
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        let rows_affected = sqlx::query(
            "UPDATE products
                SET stock_quantity = stock_quantity + $1,
                    updated_at = $2,
                    synced = false
              WHERE company_id = $3
                AND id = $4
                AND deleted_at IS NULL
                AND unlimited_stock = false
                AND stock_quantity + $1 >= 0",
        )
        .bind(delta)
        .bind(now)
        .bind(company_id)
        .bind(product_id)
        .execute(&mut *tx)
        .await
        .map_err(map_db)?
        .rows_affected();
        if rows_affected == 1 {
            // Ledger append-only na MESMA transação (§7): propaga o delta
            // aos desktops via pull idempotente.
            insert_stock_movement(&mut tx, company_id, product_id, delta, "adjust", None, now)
                .await?;
            tx.commit().await.map_err(map_db)?;
            return Ok(StockAdjustResult::Adjusted);
        }
        tx.rollback().await.map_err(map_db)?;
        // Distingue entre Unlimited, Insufficient e NotFound.
        let row: Option<(bool, f64, Option<chrono::NaiveDateTime>)> = sqlx::query_as(
            "SELECT unlimited_stock, stock_quantity, deleted_at FROM products
              WHERE company_id = $1 AND id = $2",
        )
        .bind(company_id)
        .bind(product_id)
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
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28)
             ON CONFLICT (id) DO UPDATE SET
                 name = EXCLUDED.name,
                 description = EXCLUDED.description,
                 category_id = EXCLUDED.category_id,
                 subcategory_id = EXCLUDED.subcategory_id,
                 price = EXCLUDED.price,
                 cost_price = EXCLUDED.cost_price,
                 -- stock_quantity NÃO é sobrescrito no conflito: o estoque
                 -- evolui só pelo ledger (apply_stock_movement), evitando LWW
                 -- sobre o valor absoluto (overselling). §7.
                 unlimited_stock = EXCLUDED.unlimited_stock,
                 barcode = EXCLUDED.barcode,
                 unit = EXCLUDED.unit,
                 updated_at = EXCLUDED.updated_at,
                 deleted_at = EXCLUDED.deleted_at,
                 synced = EXCLUDED.synced,
                 active = EXCLUDED.active,
                 web_visible = EXCLUDED.web_visible,
                 balance_mode = EXCLUDED.balance_mode,
                 image_data = EXCLUDED.image_data,
                 cover_color = EXCLUDED.cover_color,
                 availability_schedule = EXCLUDED.availability_schedule,
                 discount_kind = EXCLUDED.discount_kind,
                 discount_value = EXCLUDED.discount_value,
                 discount_min_qty = EXCLUDED.discount_min_qty,
                 discount_tiers = EXCLUDED.discount_tiers,
                 variations = EXCLUDED.variations,
                 min_stock = EXCLUDED.min_stock
             WHERE EXCLUDED.updated_at > products.updated_at AND products.company_id = EXCLUDED.company_id",
        )
        .bind(product.base.id)
        .bind(product.base.company_id)
        .bind(&product.name)
        .bind(&product.description)
        .bind(product.category_id)
        .bind(product.subcategory_id)
        .bind(product.price)
        .bind(product.cost_price)
        .bind(product.stock_quantity)
        .bind(product.unlimited_stock)
        .bind(&product.barcode)
        .bind(&product.unit)
        .bind(product.base.created_at)
        .bind(product.base.updated_at)
        .bind(product.base.deleted_at)
        .bind(product.base.synced)
        .bind(product.active)
        .bind(product.web_visible)
        .bind(product.balance_mode.as_db_str())
        .bind(&product.image_data)
        .bind(&product.cover_color)
        .bind(&product.availability_schedule)
        .bind(&product.discount_kind)
        .bind(product.discount_value)
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
             FROM stock_movements WHERE company_id = $1 AND synced = false ORDER BY created_at ASC",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(StockMovement::from).collect())
    }

    async fn mark_stock_movement_synced(
        &self,
        company_id: Uuid,
        id: Uuid,
        updated_at: NaiveDateTime,
    ) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE stock_movements SET synced = true
             WHERE company_id = $1 AND id = $2 AND updated_at = $3",
        )
        .bind(company_id)
        .bind(id)
        .bind(updated_at)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn apply_stock_movement(&self, m: &StockMovement) -> Result<(), CoreError> {
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        // `ON CONFLICT DO NOTHING` garante idempotência: id repetido não
        // reinsere e o delta NÃO é reaplicado. Movimento aplicado = synced.
        let inserted = sqlx::query(
            "INSERT INTO stock_movements
                (id, company_id, product_id, delta, reason, order_id, created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, true)
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(m.base.id)
        .bind(m.base.company_id)
        .bind(m.product_id)
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
            // Aplica o delta ao materializado só na 1ª vez. Não toca
            // updated_at/synced do produto (mudança veio do sync).
            sqlx::query(
                "UPDATE products SET stock_quantity = stock_quantity + $1
                 WHERE company_id = $2 AND id = $3 AND unlimited_stock = false",
            )
            .bind(m.delta)
            .bind(m.base.company_id)
            .bind(m.product_id)
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
        since: NaiveDateTime,
    ) -> Result<Vec<StockMovement>, CoreError> {
        let rows = sqlx::query_as::<_, StockMovementRow>(
            "SELECT id, company_id, product_id, delta, reason, order_id, created_at, updated_at, deleted_at, synced
             FROM stock_movements WHERE company_id = $1 AND updated_at > $2 ORDER BY updated_at ASC",
        )
        .bind(company_id)
        .bind(since)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(StockMovement::from).collect())
    }

    async fn find_stock_movements_updated_since_paged(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
        after_id: Uuid,
        limit: i64,
    ) -> Result<Vec<StockMovement>, CoreError> {
        let rows = sqlx::query_as::<_, StockMovementRow>(
            "SELECT id, company_id, product_id, delta, reason, order_id, created_at, updated_at, deleted_at, synced
               FROM stock_movements
              WHERE company_id = $1
                AND (updated_at > $2 OR (updated_at = $2 AND id > $3))
              ORDER BY updated_at ASC, id ASC
              LIMIT $4",
        )
        .bind(company_id)
        .bind(since)
        .bind(after_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(StockMovement::from).collect())
    }
}
