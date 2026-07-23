use async_trait::async_trait;
use rust_decimal::Decimal;
use chrono::NaiveDateTime;
use std::collections::HashMap;
use sqlx::prelude::FromRow;
use sqlx::PgPool;
use uuid::Uuid;

use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;
use letaf_core::order::model::{DeliveryType, Order, OrderItem, OrderStatus};
use letaf_core::order::repository::OrderRepository;

use super::helpers::{insert_stock_movement, keyset_pull_sql, map_db};

/// Converte sentinela `Uuid::nil()` (= sem cliente, vinda do desktop)
/// em `None` para o bind do Postgres — evita FK violation em
/// `customers.id`. Pedidos PDV anônimos chegam com `customer_id` nil.
fn opt_customer(id: Uuid) -> Option<Uuid> {
    if id.is_nil() { None } else { Some(id) }
}

#[derive(FromRow)]
struct OrderRow {
    id: Uuid,
    company_id: Uuid,
    /// Nullable porque pedidos PDV anônimos não têm cliente. Mapeado
    /// para `Uuid::nil()` no domínio (que não é nullable) — preserva
    /// a semântica do desktop (que usa nil como sentinela).
    customer_id: Option<Uuid>,
    number: i64,
    status: String,
    total: Decimal,
    coupon_code: Option<String>,
    discount_amount: Decimal,
    additional_amount: Decimal,
    delivery_type: String,
    notes: Option<String>,
    cancellation_reason: Option<String>,
    payment_method: Option<String>,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    deleted_at: Option<NaiveDateTime>,
    synced: bool,
}

impl From<OrderRow> for Order {
    fn from(r: OrderRow) -> Self {
        Self {
            base: BaseFields {
                id: r.id,
                company_id: r.company_id,
                created_at: r.created_at,
                updated_at: r.updated_at,
                deleted_at: r.deleted_at,
                synced: r.synced,
            },
            customer_id: r.customer_id.unwrap_or(Uuid::nil()),
            number: r.number,
            status: OrderStatus::from_str(&r.status).unwrap_or_else(|| {
                tracing::warn!("Status de pedido desconhecido no banco: {:?} (id={}); usando Pending", r.status, r.id);
                OrderStatus::Pending
            }),
            total: r.total,
            coupon_code: r.coupon_code,
            discount_amount: r.discount_amount,
            additional_amount: r.additional_amount,
            delivery_type: DeliveryType::from_str(&r.delivery_type),
            notes: r.notes,
            cancellation_reason: r.cancellation_reason,
            payment_method: r.payment_method,
            items: Vec::new(),
        }
    }
}

#[derive(FromRow)]
struct OrderItemRow {
    id: Uuid,
    company_id: Uuid,
    order_id: Uuid,
    product_id: Uuid,
    product_name: String,
    quantity: f64,
    unit_price: Decimal,
    subtotal: Decimal,
    notes: Option<String>,
    addons_json: Option<String>,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    deleted_at: Option<NaiveDateTime>,
    synced: bool,
}

impl From<OrderItemRow> for OrderItem {
    fn from(r: OrderItemRow) -> Self {
        Self {
            base: BaseFields {
                id: r.id,
                company_id: r.company_id,
                created_at: r.created_at,
                updated_at: r.updated_at,
                deleted_at: r.deleted_at,
                synced: r.synced,
            },
            order_id: r.order_id,
            product_id: r.product_id,
            product_name: r.product_name,
            quantity: r.quantity,
            unit_price: r.unit_price,
            subtotal: r.subtotal,
            notes: r.notes,
            addons_json: r.addons_json,
        }
    }
}

/// Implementação PostgreSQL do OrderRepository.
///
/// Regras aplicadas (AI_RULES.md §3, §5, §6, §10):
/// - Todas queries filtram por company_id (isolamento)
/// - Soft delete via deleted_at
/// - Servidor usa PostgreSQL
/// - Order e OrderItem gerenciados em transação
pub struct PgOrderRepository {
    pool: PgPool,
}

impl PgOrderRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn load_items(&self, company_id: Uuid, order_id: Uuid) -> Result<Vec<OrderItem>, CoreError> {
        let rows = sqlx::query_as::<_, OrderItemRow>(
            "SELECT * FROM order_items WHERE company_id = $1 AND order_id = $2 AND deleted_at IS NULL ORDER BY created_at",
        )
        .bind(company_id)
        .bind(order_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(rows.into_iter().map(OrderItem::from).collect())
    }

    /// Carrega itens de múltiplos pedidos em uma única query (`= ANY`), eliminando N+1.
    ///
    /// Regras aplicadas (AI_RULES.md §8, §9):
    /// - Substituição do loop N queries por 1 query batched.
    async fn load_items_batch(
        &self,
        company_id: Uuid,
        order_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, Vec<OrderItem>>, CoreError> {
        if order_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let rows = sqlx::query_as::<_, OrderItemRow>(
            "SELECT * FROM order_items \
             WHERE company_id = $1 AND order_id = ANY($2) AND deleted_at IS NULL \
             ORDER BY created_at",
        )
        .bind(company_id)
        .bind(order_ids)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        let mut map: HashMap<Uuid, Vec<OrderItem>> = HashMap::new();
        for item in rows.into_iter().map(OrderItem::from) {
            map.entry(item.order_id).or_default().push(item);
        }
        Ok(map)
    }

    /// Anexa os itens (1 query batch) a uma lista de pedidos já carregados.
    async fn attach_items(&self, orders: &mut [Order]) -> Result<(), CoreError> {
        if orders.is_empty() {
            return Ok(());
        }
        let company_id = orders[0].base.company_id;
        let ids: Vec<Uuid> = orders.iter().map(|o| o.base.id).collect();
        let mut map = self.load_items_batch(company_id, &ids).await?;
        for order in orders.iter_mut() {
            order.items = map.remove(&order.base.id).unwrap_or_default();
        }
        Ok(())
    }
}

#[async_trait]
impl OrderRepository for PgOrderRepository {
    async fn next_number(&self, company_id: Uuid) -> Result<i64, CoreError> {
        // MAX+1 escopado por company_id (§6, §11). Inclui pedidos soft-deleted
        // para evitar reuso de números de pedidos cancelados/removidos.
        let row: (Option<i64>,) = sqlx::query_as(
            "SELECT MAX(number) FROM orders WHERE company_id = $1",
        )
        .bind(company_id)
        .fetch_one(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(row.0.unwrap_or(0) + 1)
    }

    async fn create_atomic(
        &self,
        order: &Order,
        stock_deltas: &[(Uuid, f64)],
    ) -> Result<(), CoreError> {
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        let now = chrono::Utc::now().naive_utc();

        // 1) Baixa de estoque na MESMA transação (§4). `unlimited_stock`
        // não decrementa; estoque insuficiente ou produto inexistente
        // abortam a tx (rollback automático ao dropar `tx`).
        for (product_id, qty) in stock_deltas {
            let rows = sqlx::query(
                "UPDATE products
                    SET stock_quantity = stock_quantity - $1, updated_at = $2, synced = false
                  WHERE company_id = $3 AND id = $4 AND deleted_at IS NULL
                    AND unlimited_stock = false AND stock_quantity - $1 >= 0",
            )
            .bind(qty)
            .bind(now)
            .bind(order.base.company_id)
            .bind(product_id)
            .execute(&mut *tx)
            .await
            .map_err(map_db)?
            .rows_affected();
            if rows == 0 {
                // Distingue unlimited (ok) / insufficient / notfound — na tx.
                let row: Option<(bool, Option<chrono::NaiveDateTime>, String)> = sqlx::query_as(
                    "SELECT unlimited_stock, deleted_at, name FROM products
                      WHERE company_id = $1 AND id = $2",
                )
                .bind(order.base.company_id)
                .bind(product_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(map_db)?;
                match row {
                    None | Some((_, Some(_), _)) => {
                        return Err(CoreError::NotFound(format!("Product not found: {product_id}")));
                    }
                    Some((true, _, _)) => { /* ilimitado: nada a decrementar */ }
                    Some((false, _, name)) => {
                        return Err(CoreError::Validation(format!(
                            "Estoque insuficiente para '{name}' (necessário: {qty})"
                        )));
                    }
                }
            } else {
                // Decremento efetivado → registra o delta no ledger (§7), na
                // MESMA transação, para propagar aos desktops via pull.
                insert_stock_movement(
                    &mut tx,
                    order.base.company_id,
                    *product_id,
                    -*qty,
                    "sale",
                    Some(order.base.id),
                    now,
                )
                .await?;
            }
        }

        // 2) Insere o pedido + itens (mesma tx).
        sqlx::query(
            "INSERT INTO orders (id, company_id, customer_id, number, status, total, delivery_type, notes, cancellation_reason, created_at, updated_at, deleted_at, synced, coupon_code, discount_amount, additional_amount, payment_method)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)",
        )
        .bind(order.base.id)
        .bind(order.base.company_id)
        .bind(opt_customer(order.customer_id))
        .bind(order.number)
        .bind(order.status.to_string())
        .bind(order.total)
        .bind(order.delivery_type.to_string())
        .bind(&order.notes)
        .bind(&order.cancellation_reason)
        .bind(order.base.created_at)
        .bind(order.base.updated_at)
        .bind(order.base.deleted_at)
        .bind(order.base.synced)
        .bind(&order.coupon_code)
        .bind(order.discount_amount)
        .bind(order.additional_amount)
        .bind(&order.payment_method)
        .execute(&mut *tx)
        .await
        .map_err(map_db)?;

        for item in &order.items {
            sqlx::query(
                "INSERT INTO order_items (id, company_id, order_id, product_id, product_name, quantity, unit_price, subtotal, notes, addons_json, created_at, updated_at, deleted_at, synced)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
            )
            .bind(item.base.id)
            .bind(item.base.company_id)
            .bind(item.order_id)
            .bind(item.product_id)
            .bind(&item.product_name)
            .bind(item.quantity)
            .bind(item.unit_price)
            .bind(item.subtotal)
            .bind(&item.notes)
            .bind(&item.addons_json)
            .bind(item.base.created_at)
            .bind(item.base.updated_at)
            .bind(item.base.deleted_at)
            .bind(item.base.synced)
            .execute(&mut *tx)
            .await
            .map_err(map_db)?;
        }

        tx.commit().await.map_err(map_db)?;
        Ok(())
    }

    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Order>, CoreError> {
        let row = sqlx::query_as::<_, OrderRow>(
            "SELECT * FROM orders WHERE company_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(company_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;

        match row {
            Some(r) => {
                let mut order = Order::from(r);
                order.items = self.load_items(order.base.company_id, order.base.id).await?;
                Ok(Some(order))
            }
            None => Ok(None),
        }
    }

    /// Override eficiente: `COUNT(*)` em vez de materializar as linhas (§13).
    async fn count_all(&self, company_id: Uuid) -> Result<i64, CoreError> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM orders WHERE company_id = $1 AND deleted_at IS NULL",
        )
        .bind(company_id)
        .fetch_one(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(row.0)
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<Order>, CoreError> {
        let rows = sqlx::query_as::<_, OrderRow>(
            "SELECT * FROM orders WHERE company_id = $1 AND deleted_at IS NULL ORDER BY created_at DESC",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        let mut orders: Vec<Order> = rows.into_iter().map(Order::from).collect();
        if !orders.is_empty() {
            let ids: Vec<Uuid> = orders.iter().map(|o| o.base.id).collect();
            let mut map = self.load_items_batch(company_id, &ids).await?;
            for order in &mut orders {
                order.items = map.remove(&order.base.id).unwrap_or_default();
            }
        }
        Ok(orders)
    }

    async fn find_all_paged(
        &self,
        company_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Order>, CoreError> {
        let rows = sqlx::query_as::<_, OrderRow>(
            "SELECT * FROM orders WHERE company_id = $1 AND deleted_at IS NULL
             ORDER BY created_at DESC LIMIT $2 OFFSET $3",
        )
        .bind(company_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        let mut orders: Vec<Order> = rows.into_iter().map(Order::from).collect();
        if !orders.is_empty() {
            let ids: Vec<Uuid> = orders.iter().map(|o| o.base.id).collect();
            let mut map = self.load_items_batch(company_id, &ids).await?;
            for order in &mut orders {
                order.items = map.remove(&order.base.id).unwrap_or_default();
            }
        }
        Ok(orders)
    }

    async fn find_by_customer(&self, company_id: Uuid, customer_id: Uuid) -> Result<Vec<Order>, CoreError> {
        let rows = sqlx::query_as::<_, OrderRow>(
            "SELECT * FROM orders WHERE company_id = $1 AND customer_id = $2 AND deleted_at IS NULL ORDER BY created_at DESC",
        )
        .bind(company_id)
        .bind(customer_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        let mut orders: Vec<Order> = rows.into_iter().map(Order::from).collect();
        if !orders.is_empty() {
            let ids: Vec<Uuid> = orders.iter().map(|o| o.base.id).collect();
            let mut map = self.load_items_batch(company_id, &ids).await?;
            for order in &mut orders {
                order.items = map.remove(&order.base.id).unwrap_or_default();
            }
        }
        Ok(orders)
    }

    async fn count_coupon_uses(&self, company_id: Uuid, coupon_code: &str) -> Result<i64, CoreError> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM orders
             WHERE company_id = $1 AND UPPER(coupon_code) = UPPER($2)
               AND status <> 'cancelled' AND deleted_at IS NULL",
        )
        .bind(company_id)
        .bind(coupon_code)
        .fetch_one(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(row.0)
    }

    async fn count_customer_orders(&self, company_id: Uuid, customer_id: Uuid) -> Result<i64, CoreError> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM orders
             WHERE company_id = $1 AND customer_id = $2
               AND status <> 'cancelled' AND deleted_at IS NULL",
        )
        .bind(company_id)
        .bind(customer_id)
        .fetch_one(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(row.0)
    }

    async fn count_customer_coupon_uses(
        &self,
        company_id: Uuid,
        customer_id: Uuid,
        coupon_code: &str,
    ) -> Result<i64, CoreError> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM orders
             WHERE company_id = $1 AND customer_id = $2 AND UPPER(coupon_code) = UPPER($3)
               AND status <> 'cancelled' AND deleted_at IS NULL",
        )
        .bind(company_id)
        .bind(customer_id)
        .bind(coupon_code)
        .fetch_one(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(row.0)
    }

    async fn find_by_status(&self, company_id: Uuid, status: &OrderStatus) -> Result<Vec<Order>, CoreError> {
        let rows = sqlx::query_as::<_, OrderRow>(
            "SELECT * FROM orders WHERE company_id = $1 AND status = $2 AND deleted_at IS NULL ORDER BY created_at DESC",
        )
        .bind(company_id)
        .bind(status.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        let mut orders: Vec<Order> = rows.into_iter().map(Order::from).collect();
        if !orders.is_empty() {
            let ids: Vec<Uuid> = orders.iter().map(|o| o.base.id).collect();
            let mut map = self.load_items_batch(company_id, &ids).await?;
            for order in &mut orders {
                order.items = map.remove(&order.base.id).unwrap_or_default();
            }
        }
        Ok(orders)
    }

    async fn update_status(&self, company_id: Uuid, id: Uuid, status: &OrderStatus) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        sqlx::query(
            "UPDATE orders SET status = $1, updated_at = $2, synced = false
             WHERE company_id = $3 AND id = $4 AND deleted_at IS NULL",
        )
        .bind(status.to_string())
        .bind(now)
        .bind(company_id)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    async fn update_atomic(
        &self,
        order: &Order,
        stock_deltas: &[(Uuid, f64)],
    ) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        let mut tx = self.pool.begin().await.map_err(map_db)?;

        // Ajuste de estoque na MESMA transação (§4). `delta` soma ao estoque:
        // negativo baixa (edição aumentou a qty) → checa suficiência; positivo
        // restitui (qty diminuiu). Ilimitado/excluído é pulado; insuficiente
        // aborta a tx (nada persiste).
        for (product_id, delta) in stock_deltas {
            if delta.abs() < f64::EPSILON {
                continue;
            }
            let rows = sqlx::query(
                "UPDATE products
                    SET stock_quantity = stock_quantity + $1, updated_at = $2, synced = false
                  WHERE company_id = $3 AND id = $4 AND deleted_at IS NULL
                    AND unlimited_stock = false AND stock_quantity + $1 >= 0",
            )
            .bind(delta)
            .bind(now)
            .bind(order.base.company_id)
            .bind(product_id)
            .execute(&mut *tx)
            .await
            .map_err(map_db)?
            .rows_affected();
            if rows == 0 {
                let row: Option<(bool, Option<chrono::NaiveDateTime>, String)> = sqlx::query_as(
                    "SELECT unlimited_stock, deleted_at, name FROM products
                      WHERE company_id = $1 AND id = $2",
                )
                .bind(order.base.company_id)
                .bind(product_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(map_db)?;
                match row {
                    // Produto excluído/inexistente: nada a ajustar (não aborta a edição).
                    None | Some((_, Some(_), _)) => {}
                    Some((true, _, _)) => { /* ilimitado: nada a ajustar */ }
                    Some((false, _, name)) => {
                        // Rastreia estoque e não deu para baixar → insuficiente.
                        return Err(CoreError::Validation(format!(
                            "Estoque insuficiente para '{name}' na edição do pedido"
                        )));
                    }
                }
            } else {
                insert_stock_movement(&mut tx, order.base.company_id, *product_id, *delta, "edit", Some(order.base.id), now)
                    .await?;
            }
        }

        sqlx::query(
            "DELETE FROM order_items WHERE company_id = $1 AND order_id = $2",
        )
        .bind(order.base.company_id)
        .bind(order.base.id)
        .execute(&mut *tx)
        .await
        .map_err(map_db)?;
        for item in &order.items {
            sqlx::query(
                "INSERT INTO order_items (id, company_id, order_id, product_id, product_name, quantity, unit_price, subtotal, notes, addons_json, created_at, updated_at, deleted_at, synced)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
            )
            .bind(item.base.id)
            .bind(item.base.company_id)
            .bind(item.order_id)
            .bind(item.product_id)
            .bind(&item.product_name)
            .bind(item.quantity)
            .bind(item.unit_price)
            .bind(item.subtotal)
            .bind(&item.notes)
            .bind(&item.addons_json)
            .bind(item.base.created_at)
            .bind(item.base.updated_at)
            .bind(item.base.deleted_at)
            .bind(item.base.synced)
            .execute(&mut *tx)
            .await
            .map_err(map_db)?;
        }
        sqlx::query(
            "UPDATE orders SET total = $1, notes = $2, delivery_type = $3,
                    updated_at = $4, synced = false
             WHERE company_id = $5 AND id = $6 AND deleted_at IS NULL",
        )
        .bind(order.total)
        .bind(order.notes.as_deref())
        .bind(order.delivery_type.to_string())
        .bind(now)
        .bind(order.base.company_id)
        .bind(order.base.id)
        .execute(&mut *tx)
        .await
        .map_err(map_db)?;
        tx.commit().await.map_err(map_db)?;
        Ok(())
    }

    async fn cancel_atomic(
        &self,
        company_id: Uuid,
        id: Uuid,
        reason: &str,
        restitutions: &[(Uuid, f64)],
    ) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        let mut tx = self.pool.begin().await.map_err(map_db)?;

        sqlx::query(
            "UPDATE orders SET status = 'cancelled', cancellation_reason = $1, updated_at = $2, synced = false
             WHERE company_id = $3 AND id = $4 AND deleted_at IS NULL",
        )
        .bind(reason)
        .bind(now)
        .bind(company_id)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(map_db)?;

        // Restitui o estoque (+qty). Produto ilimitado/excluído → rows=0 →
        // pula sem erro (nada a devolver); o cancelamento não falha por isso.
        for (product_id, qty) in restitutions {
            let rows = sqlx::query(
                "UPDATE products
                    SET stock_quantity = stock_quantity + $1, updated_at = $2, synced = false
                  WHERE company_id = $3 AND id = $4 AND deleted_at IS NULL
                    AND unlimited_stock = false",
            )
            .bind(qty)
            .bind(now)
            .bind(company_id)
            .bind(product_id)
            .execute(&mut *tx)
            .await
            .map_err(map_db)?
            .rows_affected();
            if rows > 0 {
                insert_stock_movement(&mut tx, company_id, *product_id, *qty, "cancel", Some(id), now)
                    .await?;
            }
        }

        tx.commit().await.map_err(map_db)?;
        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        let mut tx = self.pool.begin().await.map_err(map_db)?;

        sqlx::query(
            "UPDATE orders SET deleted_at = $1, updated_at = $2, synced = false
             WHERE company_id = $3 AND id = $4 AND deleted_at IS NULL",
        )
        .bind(now)
        .bind(now)
        .bind(company_id)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(map_db)?;

        sqlx::query(
            "UPDATE order_items SET deleted_at = $1, updated_at = $2, synced = false
             WHERE company_id = $3 AND order_id = $4 AND deleted_at IS NULL",
        )
        .bind(now)
        .bind(now)
        .bind(company_id)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(map_db)?;

        tx.commit().await.map_err(map_db)?;
        Ok(())
    }

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Order>, CoreError> {
        let rows = sqlx::query_as::<_, OrderRow>(
            "SELECT * FROM orders WHERE company_id = $1 AND synced = false",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        let mut orders: Vec<Order> = rows.into_iter().map(Order::from).collect();
        if !orders.is_empty() {
            let ids: Vec<Uuid> = orders.iter().map(|o| o.base.id).collect();
            let mut map = self.load_items_batch(company_id, &ids).await?;
            for order in &mut orders {
                order.items = map.remove(&order.base.id).unwrap_or_default();
            }
        }
        Ok(orders)
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query("UPDATE orders SET synced = true WHERE company_id = $1 AND id = $2 AND updated_at = $3")
            .bind(company_id)
            .bind(id)
        .bind(updated_at)
            .execute(&self.pool)
            .await
            .map_err(map_db)?;

        Ok(())
    }

    async fn find_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<Order>, CoreError> {
        let rows = sqlx::query_as::<_, OrderRow>(
            "SELECT * FROM orders WHERE company_id = $1 AND updated_at > $2",
        )
        .bind(company_id)
        .bind(since)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        let mut orders: Vec<Order> = rows.into_iter().map(Order::from).collect();
        self.attach_items(&mut orders).await?;
        Ok(orders)
    }

    async fn find_updated_since_paged(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
        after_id: Uuid,
        limit: i64,
    ) -> Result<Vec<Order>, CoreError> {
        let rows = sqlx::query_as::<_, OrderRow>(&keyset_pull_sql("orders"))
        .bind(company_id)
        .bind(since)
        .bind(after_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        let mut orders: Vec<Order> = rows.into_iter().map(Order::from).collect();
        self.attach_items(&mut orders).await?;
        Ok(orders)
    }

    async fn sync_upsert(&self, order: &Order) -> Result<(), CoreError> {
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        // Mesmo padrão do desktop: se o incoming vence o last-write-wins,
        // REESCREVE a lista de items (DELETE + INSERT). Sem isto, items
        // removidos pelo cliente nunca somem do banco (upsert_item só
        // sabe inserir/atualizar, nunca apagar).
        let existing: Option<(chrono::NaiveDateTime,)> = sqlx::query_as(
            "SELECT updated_at FROM orders WHERE id = $1"
        )
        .bind(order.base.id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_db)?;
        let incoming_wins = match existing.as_ref().map(|(d,)| *d) {
            Some(local) => order.base.updated_at > local,
            None => true,
        };
        // Colisão de `number` entre origens (desktop offline × web): o número é
        // MAX+1 independente nos dois bancos. Se este id é NOVO aqui e o número
        // já pertence a OUTRO pedido, renumera para o próximo livre e bumpa
        // `updated_at` — assim o desktop reconverte no pull. Sem isto o INSERT
        // viola o UNIQUE(company_id, number) e o push re-tentaria para sempre,
        // travando o sync e perdendo a venda no servidor (§7.6). Cada tentativa
        // recomputa MAX+1, então converge mesmo sob corrida.
        let renumbered: Option<Order> = if existing.is_none() {
            let clash: Option<(uuid::Uuid,)> = sqlx::query_as(
                "SELECT id FROM orders WHERE company_id = $1 AND number = $2 AND id <> $3 LIMIT 1",
            )
            .bind(order.base.company_id)
            .bind(order.number)
            .bind(order.base.id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_db)?;
            if clash.is_some() {
                let (next,): (i64,) = sqlx::query_as(
                    "SELECT COALESCE(MAX(number), 0) + 1 FROM orders WHERE company_id = $1",
                )
                .bind(order.base.company_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(map_db)?;
                let mut o = order.clone();
                o.number = next;
                o.base.updated_at = chrono::Utc::now().naive_utc();
                Some(o)
            } else {
                None
            }
        } else {
            None
        };
        let order = renumbered.as_ref().unwrap_or(order);
        upsert_order_row(&mut tx, order).await?;
        if incoming_wins {
            sqlx::query(
                "DELETE FROM order_items WHERE company_id = $1 AND order_id = $2"
            )
            .bind(order.base.company_id)
            .bind(order.base.id)
            .execute(&mut *tx)
            .await
            .map_err(map_db)?;
            for item in &order.items {
                // Mantém o upsert para preservar idempotência mesmo após
                // o delete (caso o vetor traga IDs repetidos por bug
                // remoto). O DELETE acima garante reconciliação.
                upsert_order_item_row(&mut tx, item).await?;
            }
        }
        tx.commit().await.map_err(map_db)?;
        Ok(())
    }
}

/// Upsert da linha de `Order` aplicando last-write-wins por `updated_at` (§7.7).
async fn upsert_order_row(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    order: &Order,
) -> Result<(), CoreError> {
    sqlx::query(
        "INSERT INTO orders (id, company_id, customer_id, number, status, total, delivery_type, notes, cancellation_reason, created_at, updated_at, deleted_at, synced, coupon_code, discount_amount, additional_amount, payment_method)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)
         ON CONFLICT (id) DO UPDATE SET
             status = EXCLUDED.status,
             total = EXCLUDED.total,
             delivery_type = EXCLUDED.delivery_type,
             notes = EXCLUDED.notes,
             cancellation_reason = EXCLUDED.cancellation_reason,
             updated_at = EXCLUDED.updated_at,
             deleted_at = EXCLUDED.deleted_at,
             synced = EXCLUDED.synced,
             coupon_code = EXCLUDED.coupon_code,
             discount_amount = EXCLUDED.discount_amount,
             additional_amount = EXCLUDED.additional_amount,
             payment_method = EXCLUDED.payment_method
         WHERE EXCLUDED.updated_at > orders.updated_at AND orders.company_id = EXCLUDED.company_id",
    )
    .bind(order.base.id)
    .bind(order.base.company_id)
    .bind(opt_customer(order.customer_id))
    .bind(order.number)
    .bind(order.status.to_string())
    .bind(order.total)
    .bind(order.delivery_type.to_string())
    .bind(&order.notes)
    .bind(&order.cancellation_reason)
    .bind(order.base.created_at)
    .bind(order.base.updated_at)
    .bind(order.base.deleted_at)
    .bind(order.base.synced)
    .bind(&order.coupon_code)
    .bind(order.discount_amount)
    .bind(order.additional_amount)
    .bind(&order.payment_method)
    .execute(&mut **tx)
    .await
    .map_err(map_db)?;
    Ok(())
}

/// Upsert da linha de `OrderItem` aplicando last-write-wins por `updated_at` (§7.7).
async fn upsert_order_item_row(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    item: &letaf_core::order::model::OrderItem,
) -> Result<(), CoreError> {
    sqlx::query(
        "INSERT INTO order_items (id, company_id, order_id, product_id, product_name, quantity, unit_price, subtotal, notes, addons_json, created_at, updated_at, deleted_at, synced)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
         ON CONFLICT (id) DO UPDATE SET
             quantity = EXCLUDED.quantity,
             unit_price = EXCLUDED.unit_price,
             subtotal = EXCLUDED.subtotal,
             notes = EXCLUDED.notes,
             addons_json = EXCLUDED.addons_json,
             updated_at = EXCLUDED.updated_at,
             deleted_at = EXCLUDED.deleted_at,
             synced = EXCLUDED.synced
         WHERE EXCLUDED.updated_at > order_items.updated_at AND order_items.company_id = EXCLUDED.company_id",
    )
    .bind(item.base.id)
    .bind(item.base.company_id)
    .bind(item.order_id)
    .bind(item.product_id)
    .bind(&item.product_name)
    .bind(item.quantity)
    .bind(item.unit_price)
    .bind(item.subtotal)
    .bind(&item.notes)
    .bind(&item.addons_json)
    .bind(item.base.created_at)
    .bind(item.base.updated_at)
    .bind(item.base.deleted_at)
    .bind(item.base.synced)
    .execute(&mut **tx)
    .await
    .map_err(map_db)?;
    Ok(())
}
