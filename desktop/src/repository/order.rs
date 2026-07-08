use async_trait::async_trait;
use rust_decimal::prelude::ToPrimitive;
use chrono::NaiveDateTime;
use std::collections::HashMap;
use sqlx::prelude::FromRow;
use sqlx::{Sqlite, SqlitePool, Transaction};
use uuid::Uuid;

use letaf_core::error::CoreError;
use letaf_core::order::model::{DeliveryType, Order, OrderItem, OrderStatus};
use letaf_core::order::repository::OrderRepository;

use super::helpers::{parse_base, insert_stock_movement, map_db, parse_timestamp, parse_uuid, ts};

/// Implementação SQLite do `OrderRepository`.
///
/// Regras aplicadas (AI_RULES.md §3, §5, §7, §10, §11):
/// - Desktop usa SQLite (§5)
/// - Todas as queries filtram por `company_id` (§11)
/// - Soft delete via `deleted_at` (§6)
/// - Acesso ao banco somente via repository (§10)
/// - `Order` + `OrderItem` gerenciados em transação (§4)
pub struct SqliteOrderRepository {
    pool: SqlitePool,
}

impl SqliteOrderRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    async fn load_items(&self, company_id: Uuid, order_id: Uuid) -> Result<Vec<OrderItem>, CoreError> {
        let rows = sqlx::query_as::<_, OrderItemRow>(
            "SELECT * FROM order_items WHERE company_id = ?1 AND order_id = ?2 AND deleted_at IS NULL ORDER BY created_at",
        )
        .bind(company_id.to_string())
        .bind(order_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        rows.into_iter().map(OrderItem::try_from).collect()
    }

    /// Carrega itens de múltiplos pedidos em 1 query (IN dinâmico), eliminando N+1.
    ///
    /// Regras aplicadas (AI_RULES.md §8, §9):
    /// - Cada `?` é um parâmetro posicional; gerado dinamicamente mas sem risco
    ///   de injeção pois todos os valores são UUIDs gerados internamente.
    async fn load_items_batch(
        &self,
        company_id: Uuid,
        order_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, Vec<OrderItem>>, CoreError> {
        if order_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let placeholders = vec!["?"; order_ids.len()].join(", ");
        let sql = format!(
            "SELECT * FROM order_items WHERE company_id = ? AND order_id IN ({}) AND deleted_at IS NULL ORDER BY created_at",
            placeholders
        );
        let mut query = sqlx::query_as::<_, OrderItemRow>(&sql).bind(company_id.to_string());
        for id in order_ids {
            query = query.bind(id.to_string());
        }
        let rows = query.fetch_all(&self.pool).await.map_err(map_db)?;

        let mut map: HashMap<Uuid, Vec<OrderItem>> = HashMap::new();
        for row in rows {
            let item = OrderItem::try_from(row)?;
            let oid = item.order_id;
            map.entry(oid).or_default().push(item);
        }
        Ok(map)
    }

    /// Substitui o loop N+1 usando load_items_batch.
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
impl OrderRepository for SqliteOrderRepository {
    async fn next_number(&self, company_id: Uuid) -> Result<i64, CoreError> {
        // MAX+1 escopado por company_id (§6, §11). Inclui pedidos soft-deleted
        // para evitar reuso de números.
        let row: (Option<i64>,) = sqlx::query_as(
            "SELECT MAX(number) FROM orders WHERE company_id = ?1",
        )
        .bind(company_id.to_string())
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
        let now = ts(chrono::Utc::now().naive_utc());

        // Baixa de estoque na MESMA transação (§4). `unlimited_stock`
        // não decrementa; insuficiente/inexistente aborta (rollback no drop).
        for (product_id, qty) in stock_deltas {
            let rows = sqlx::query(
                "UPDATE products
                    SET stock_quantity = stock_quantity - ?1, updated_at = ?2, synced = 0
                  WHERE company_id = ?3 AND id = ?4 AND deleted_at IS NULL
                    AND unlimited_stock = 0 AND stock_quantity - ?1 >= 0",
            )
            .bind(qty)
            .bind(&now)
            .bind(order.base.company_id.to_string())
            .bind(product_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(map_db)?
            .rows_affected();
            if rows == 0 {
                let row: Option<(bool, Option<String>, String)> = sqlx::query_as(
                    "SELECT unlimited_stock, deleted_at, name FROM products
                      WHERE company_id = ?1 AND id = ?2",
                )
                .bind(order.base.company_id.to_string())
                .bind(product_id.to_string())
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
                // MESMA transação, base do sync idempotente de estoque.
                insert_stock_movement(
                    &mut tx,
                    order.base.company_id,
                    *product_id,
                    -*qty,
                    "sale",
                    Some(order.base.id),
                    &now,
                )
                .await?;
            }
        }

        insert_order(&mut tx, order).await?;
        for item in &order.items {
            insert_item(&mut tx, item).await?;
        }
        tx.commit().await.map_err(map_db)?;
        Ok(())
    }

    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Order>, CoreError> {
        let row = sqlx::query_as::<_, OrderRow>(
            "SELECT * FROM orders WHERE company_id = ?1 AND id = ?2 AND deleted_at IS NULL",
        )
        .bind(company_id.to_string())
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;

        match row {
            Some(r) => {
                let mut order = Order::try_from(r)?;
                order.items = self.load_items(order.base.company_id, order.base.id).await?;
                Ok(Some(order))
            }
            None => Ok(None),
        }
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<Order>, CoreError> {
        let rows = sqlx::query_as::<_, OrderRow>(
            "SELECT * FROM orders WHERE company_id = ?1 AND deleted_at IS NULL ORDER BY created_at DESC",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        let mut orders: Vec<Order> = rows
            .into_iter()
            .map(Order::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        self.attach_items(&mut orders).await?;
        Ok(orders)
    }

    async fn find_by_customer(&self, company_id: Uuid, customer_id: Uuid) -> Result<Vec<Order>, CoreError> {
        let rows = sqlx::query_as::<_, OrderRow>(
            "SELECT * FROM orders WHERE company_id = ?1 AND customer_id = ?2 AND deleted_at IS NULL ORDER BY created_at DESC",
        )
        .bind(company_id.to_string())
        .bind(customer_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        let mut orders: Vec<Order> = rows
            .into_iter()
            .map(Order::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        self.attach_items(&mut orders).await?;
        Ok(orders)
    }

    async fn count_coupon_uses(&self, company_id: Uuid, coupon_code: &str) -> Result<i64, CoreError> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM orders
             WHERE company_id = ?1 AND UPPER(coupon_code) = UPPER(?2)
               AND status <> 'cancelled' AND deleted_at IS NULL",
        )
        .bind(company_id.to_string())
        .bind(coupon_code)
        .fetch_one(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(row.0)
    }

    async fn count_customer_orders(&self, company_id: Uuid, customer_id: Uuid) -> Result<i64, CoreError> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM orders
             WHERE company_id = ?1 AND customer_id = ?2
               AND status <> 'cancelled' AND deleted_at IS NULL",
        )
        .bind(company_id.to_string())
        .bind(customer_id.to_string())
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
             WHERE company_id = ?1 AND customer_id = ?2 AND UPPER(coupon_code) = UPPER(?3)
               AND status <> 'cancelled' AND deleted_at IS NULL",
        )
        .bind(company_id.to_string())
        .bind(customer_id.to_string())
        .bind(coupon_code)
        .fetch_one(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(row.0)
    }

    async fn find_by_status(&self, company_id: Uuid, status: &OrderStatus) -> Result<Vec<Order>, CoreError> {
        let rows = sqlx::query_as::<_, OrderRow>(
            "SELECT * FROM orders WHERE company_id = ?1 AND status = ?2 AND deleted_at IS NULL ORDER BY created_at DESC",
        )
        .bind(company_id.to_string())
        .bind(status.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        let mut orders: Vec<Order> = rows
            .into_iter()
            .map(Order::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        self.attach_items(&mut orders).await?;
        Ok(orders)
    }

    async fn update_status(&self, company_id: Uuid, id: Uuid, status: &OrderStatus) -> Result<(), CoreError> {
        let now = ts(chrono::Utc::now().naive_utc());
        sqlx::query(
            "UPDATE orders SET status = ?1, updated_at = ?2, synced = false
             WHERE company_id = ?3 AND id = ?4 AND deleted_at IS NULL",
        )
        .bind(status.to_string())
        .bind(now)
        .bind(company_id.to_string())
        .bind(id.to_string())
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
        let now = ts(chrono::Utc::now().naive_utc());
        let mut tx = self.pool.begin().await.map_err(map_db)?;

        // Ajuste de estoque na MESMA transação (§4). `delta` soma ao estoque
        // (negativo baixa, positivo restitui); ilimitado/excluído pulado;
        // insuficiente num delta negativo aborta a tx.
        for (product_id, delta) in stock_deltas {
            if delta.abs() < f64::EPSILON {
                continue;
            }
            let rows = sqlx::query(
                "UPDATE products
                    SET stock_quantity = stock_quantity + ?1, updated_at = ?2, synced = 0
                  WHERE company_id = ?3 AND id = ?4 AND deleted_at IS NULL
                    AND unlimited_stock = 0 AND stock_quantity + ?1 >= 0",
            )
            .bind(delta)
            .bind(&now)
            .bind(order.base.company_id.to_string())
            .bind(product_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(map_db)?
            .rows_affected();
            if rows == 0 {
                let row: Option<(bool, Option<String>, String)> = sqlx::query_as(
                    "SELECT unlimited_stock, deleted_at, name FROM products
                      WHERE company_id = ?1 AND id = ?2",
                )
                .bind(order.base.company_id.to_string())
                .bind(product_id.to_string())
                .fetch_optional(&mut *tx)
                .await
                .map_err(map_db)?;
                match row {
                    None | Some((_, Some(_), _)) => {}
                    Some((true, _, _)) => {}
                    Some((false, _, name)) => {
                        return Err(CoreError::Validation(format!(
                            "Estoque insuficiente para '{name}' na edição do pedido"
                        )));
                    }
                }
            } else {
                insert_stock_movement(&mut tx, order.base.company_id, *product_id, *delta, "edit", Some(order.base.id), &now)
                    .await?;
            }
        }

        // Substitui completamente a lista de itens (delete + insert; UUIDs dos
        // items mudam entre edições — ok, são snapshots sem FK externa).
        sqlx::query(
            "DELETE FROM order_items WHERE company_id = ?1 AND order_id = ?2",
        )
        .bind(order.base.company_id.to_string())
        .bind(order.base.id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(map_db)?;
        for item in &order.items {
            insert_item(&mut tx, item).await?;
        }
        sqlx::query(
            "UPDATE orders SET total = ?1, notes = ?2, delivery_type = ?3,
                    updated_at = ?4, synced = false
             WHERE company_id = ?5 AND id = ?6 AND deleted_at IS NULL",
        )
        .bind(order.total.to_f64().unwrap_or(0.0))
        .bind(order.notes.as_deref())
        .bind(order.delivery_type.to_string())
        .bind(&now)
        .bind(order.base.company_id.to_string())
        .bind(order.base.id.to_string())
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
        let now = ts(chrono::Utc::now().naive_utc());
        let mut tx = self.pool.begin().await.map_err(map_db)?;

        sqlx::query(
            "UPDATE orders SET status = 'cancelled', cancellation_reason = ?1, updated_at = ?2, synced = false
             WHERE company_id = ?3 AND id = ?4 AND deleted_at IS NULL",
        )
        .bind(reason)
        .bind(&now)
        .bind(company_id.to_string())
        .bind(id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(map_db)?;

        // Restitui o estoque (+qty). Produto ilimitado/excluído → rows=0 →
        // pula sem erro; o cancelamento não falha por causa do estoque.
        for (product_id, qty) in restitutions {
            let rows = sqlx::query(
                "UPDATE products
                    SET stock_quantity = stock_quantity + ?1, updated_at = ?2, synced = 0
                  WHERE company_id = ?3 AND id = ?4 AND deleted_at IS NULL
                    AND unlimited_stock = 0",
            )
            .bind(qty)
            .bind(&now)
            .bind(company_id.to_string())
            .bind(product_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(map_db)?
            .rows_affected();
            if rows > 0 {
                insert_stock_movement(&mut tx, company_id, *product_id, *qty, "cancel", Some(id), &now)
                    .await?;
            }
        }

        tx.commit().await.map_err(map_db)?;
        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let now = ts(chrono::Utc::now().naive_utc());
        let mut tx = self.pool.begin().await.map_err(map_db)?;

        sqlx::query(
            "UPDATE orders SET deleted_at = ?1, updated_at = ?2, synced = false
             WHERE company_id = ?3 AND id = ?4 AND deleted_at IS NULL",
        )
        .bind(&now)
        .bind(&now)
        .bind(company_id.to_string())
        .bind(id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(map_db)?;

        sqlx::query(
            "UPDATE order_items SET deleted_at = ?1, updated_at = ?2, synced = false
             WHERE company_id = ?3 AND order_id = ?4 AND deleted_at IS NULL",
        )
        .bind(&now)
        .bind(&now)
        .bind(company_id.to_string())
        .bind(id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(map_db)?;

        tx.commit().await.map_err(map_db)?;
        Ok(())
    }

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Order>, CoreError> {
        let rows = sqlx::query_as::<_, OrderRow>(
            "SELECT * FROM orders WHERE company_id = ?1 AND synced = false",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        let mut orders: Vec<Order> = rows
            .into_iter()
            .map(Order::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        self.attach_items(&mut orders).await?;
        Ok(orders)
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        let header = sqlx::query(
            "UPDATE orders SET synced = true WHERE company_id = ?1 AND id = ?2 AND updated_at = ?3",
        )
        .bind(company_id.to_string())
        .bind(id.to_string())
        .bind(ts(updated_at))
        .execute(&mut *tx)
        .await
        .map_err(map_db)?;
        // Só marca os itens quando o HEADER foi de fato marcado (condição de
        // `updated_at` casou). Se o pedido foi editado durante o push, o header
        // fica synced=false (0 linhas) e os itens NÃO podem virar synced=true,
        // senão pai e filhos divergem (§7.6).
        if header.rows_affected() == 1 {
            sqlx::query("UPDATE order_items SET synced = true WHERE company_id = ?1 AND order_id = ?2")
                .bind(company_id.to_string())
                .bind(id.to_string())
                .execute(&mut *tx)
                .await
                .map_err(map_db)?;
        }
        tx.commit().await.map_err(map_db)?;
        Ok(())
    }

    async fn find_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<Order>, CoreError> {
        let rows = sqlx::query_as::<_, OrderRow>(
            "SELECT * FROM orders WHERE company_id = ?1 AND updated_at > ?2",
        )
        .bind(company_id.to_string())
        .bind(ts(since))
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        let mut orders: Vec<Order> = rows
            .into_iter()
            .map(Order::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        self.attach_items(&mut orders).await?;
        Ok(orders)
    }

    /// Upsert com last-write-wins via `updated_at` (§7.7).
    async fn sync_upsert(&self, order: &Order) -> Result<(), CoreError> {
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        // Lê `updated_at` local para decidir o last-write-wins ANTES
        // do upsert_order (que já tem `WHERE excluded.updated_at >
        // orders.updated_at`). Sem isto, items deletados localmente
        // voltariam ao banco no pull: o upsert_item insere o item
        // antigo (que o server ainda tem) e nada nunca remove. Bug
        // observado quando o operador apaga produto no edit do
        // pedido e o item reaparece após reabrir.
        let existing: Option<(String,)> = sqlx::query_as(
            "SELECT updated_at FROM orders WHERE id = ?1"
        )
        .bind(order.base.id.to_string())
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_db)?;
        let local_updated_at = existing
            .as_ref()
            .and_then(|(s,)| parse_timestamp(s).ok());
        let incoming_wins = match local_updated_at {
            Some(local) => order.base.updated_at > local,
            None => true,
        };
        upsert_order(&mut tx, order).await?;
        if incoming_wins {
            // Quando o incoming vence, reescreve a lista de items
            // completa: delete + insert. Garante que itens removidos
            // do lado remoto desapareçam localmente.
            sqlx::query(
                "DELETE FROM order_items WHERE company_id = ?1 AND order_id = ?2"
            )
            .bind(order.base.company_id.to_string())
            .bind(order.base.id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(map_db)?;
            for item in &order.items {
                insert_item(&mut tx, item).await?;
            }
        }
        // Quando local vence: NÃO toca em items — o upsert_order
        // também não atualiza header (cláusula WHERE protege).
        tx.commit().await.map_err(map_db)?;
        Ok(())
    }
}

// ── Helpers de transação ─────────────────────────────────────────────

async fn insert_order(tx: &mut Transaction<'_, Sqlite>, order: &Order) -> Result<(), CoreError> {
    sqlx::query(
        "INSERT INTO orders (id, company_id, customer_id, number, status, total, delivery_type, notes, cancellation_reason, created_at, updated_at, deleted_at, synced, coupon_code, discount_amount, additional_amount, payment_method)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
    )
    .bind(order.base.id.to_string())
    .bind(order.base.company_id.to_string())
    .bind(order.customer_id.to_string())
    .bind(order.number)
    .bind(order.status.to_string())
    .bind(order.total.to_f64().unwrap_or(0.0))
    .bind(order.delivery_type.to_string())
    .bind(&order.notes)
    .bind(&order.cancellation_reason)
    .bind(ts(order.base.created_at))
    .bind(ts(order.base.updated_at))
    .bind(order.base.deleted_at.map(ts))
    .bind(order.base.synced)
    .bind(&order.coupon_code)
    .bind(order.discount_amount.to_f64().unwrap_or(0.0))
    .bind(order.additional_amount.to_f64().unwrap_or(0.0))
    .bind(&order.payment_method)
    .execute(&mut **tx)
    .await
    .map_err(map_db)?;
    Ok(())
}

async fn insert_item(tx: &mut Transaction<'_, Sqlite>, item: &OrderItem) -> Result<(), CoreError> {
    sqlx::query(
        "INSERT INTO order_items (id, company_id, order_id, product_id, product_name, quantity, unit_price, subtotal, notes, addons_json, created_at, updated_at, deleted_at, synced)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
    )
    .bind(item.base.id.to_string())
    .bind(item.base.company_id.to_string())
    .bind(item.order_id.to_string())
    .bind(item.product_id.to_string())
    .bind(&item.product_name)
    .bind(item.quantity)
    .bind(item.unit_price.to_f64().unwrap_or(0.0))
    .bind(item.subtotal.to_f64().unwrap_or(0.0))
    .bind(&item.notes)
    .bind(&item.addons_json)
    .bind(ts(item.base.created_at))
    .bind(ts(item.base.updated_at))
    .bind(item.base.deleted_at.map(ts))
    .bind(item.base.synced)
    .execute(&mut **tx)
    .await
    .map_err(map_db)?;
    Ok(())
}

async fn upsert_order(tx: &mut Transaction<'_, Sqlite>, order: &Order) -> Result<(), CoreError> {
    sqlx::query(
        "INSERT INTO orders (id, company_id, customer_id, number, status, total, delivery_type, notes, cancellation_reason, created_at, updated_at, deleted_at, synced, coupon_code, discount_amount, additional_amount, payment_method)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
         ON CONFLICT (id) DO UPDATE SET
             status = excluded.status,
             total = excluded.total,
             delivery_type = excluded.delivery_type,
             notes = excluded.notes,
             cancellation_reason = excluded.cancellation_reason,
             updated_at = excluded.updated_at,
             deleted_at = excluded.deleted_at,
             synced = excluded.synced,
             coupon_code = excluded.coupon_code,
             discount_amount = excluded.discount_amount,
             additional_amount = excluded.additional_amount,
             payment_method = excluded.payment_method
         WHERE excluded.updated_at > orders.updated_at",
    )
    .bind(order.base.id.to_string())
    .bind(order.base.company_id.to_string())
    .bind(order.customer_id.to_string())
    .bind(order.number)
    .bind(order.status.to_string())
    .bind(order.total.to_f64().unwrap_or(0.0))
    .bind(order.delivery_type.to_string())
    .bind(&order.notes)
    .bind(&order.cancellation_reason)
    .bind(ts(order.base.created_at))
    .bind(ts(order.base.updated_at))
    .bind(order.base.deleted_at.map(ts))
    .bind(order.base.synced)
    .bind(&order.coupon_code)
    .bind(order.discount_amount.to_f64().unwrap_or(0.0))
    .bind(order.additional_amount.to_f64().unwrap_or(0.0))
    .bind(&order.payment_method)
    .execute(&mut **tx)
    .await
    .map_err(map_db)?;
    Ok(())
}

// ── Mapeamento de linhas SQLite → entidades core ─────────────────────

#[derive(FromRow)]
struct OrderRow {
    id: String,
    company_id: String,
    customer_id: String,
    number: i64,
    status: String,
    total: f64,
    coupon_code: Option<String>,
    discount_amount: f64,
    additional_amount: f64,
    delivery_type: String,
    notes: Option<String>,
    cancellation_reason: Option<String>,
    payment_method: Option<String>,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    synced: bool,
}

impl TryFrom<OrderRow> for Order {
    type Error = CoreError;

    fn try_from(r: OrderRow) -> Result<Self, Self::Error> {
        Ok(Self {
            base: parse_base(&r.id, &r.company_id, &r.created_at, &r.updated_at, r.deleted_at.as_deref(), r.synced)?,
            customer_id: parse_uuid(&r.customer_id)?,
            number: r.number,
            status: OrderStatus::from_str(&r.status).unwrap_or_else(|| {
                tracing::warn!("Status de pedido desconhecido no banco: {:?} (id={}); usando Pending", r.status, r.id);
                OrderStatus::Pending
            }),
            total: letaf_core::money::from_db_f64(r.total),
            coupon_code: r.coupon_code,
            discount_amount: letaf_core::money::from_db_f64(r.discount_amount),
            additional_amount: letaf_core::money::from_db_f64(r.additional_amount),
            delivery_type: DeliveryType::from_str(&r.delivery_type),
            notes: r.notes,
            cancellation_reason: r.cancellation_reason,
            payment_method: r.payment_method,
            items: Vec::new(),
        })
    }
}

#[derive(FromRow)]
struct OrderItemRow {
    id: String,
    company_id: String,
    order_id: String,
    product_id: String,
    product_name: String,
    quantity: f64,
    unit_price: f64,
    subtotal: f64,
    notes: Option<String>,
    addons_json: Option<String>,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    synced: bool,
}

impl TryFrom<OrderItemRow> for OrderItem {
    type Error = CoreError;

    fn try_from(r: OrderItemRow) -> Result<Self, Self::Error> {
        Ok(Self {
            base: parse_base(&r.id, &r.company_id, &r.created_at, &r.updated_at, r.deleted_at.as_deref(), r.synced)?,
            order_id: parse_uuid(&r.order_id)?,
            product_id: parse_uuid(&r.product_id)?,
            product_name: r.product_name,
            quantity: r.quantity,
            unit_price: letaf_core::money::from_db_f64(r.unit_price),
            subtotal: letaf_core::money::from_db_f64(r.subtotal),
            notes: r.notes,
            addons_json: r.addons_json,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn mem_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    async fn insert_product(pool: &SqlitePool, cid: Uuid, id: Uuid, stock: f64, unlimited: bool) {
        sqlx::query(
            "INSERT INTO products (id, company_id, name, price, stock_quantity, min_stock, \
             unlimited_stock, unit, created_at, updated_at, synced, active, web_visible, balance_mode) \
             VALUES (?1, ?2, 'P', NULL, ?3, 0, ?4, 'un', '2026-01-01 00:00:00.000000', \
             '2026-01-01 00:00:00.000000', 1, 1, 1, 'weight')",
        )
        .bind(id.to_string())
        .bind(cid.to_string())
        .bind(stock)
        .bind(unlimited)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn stock_and_movements(pool: &SqlitePool, id: Uuid) -> (f64, i64) {
        let stock: f64 = sqlx::query_scalar("SELECT stock_quantity FROM products WHERE id = ?1")
            .bind(id.to_string())
            .fetch_one(pool)
            .await
            .unwrap();
        let mv: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM stock_movements WHERE product_id = ?1 AND reason = 'cancel'",
        )
        .bind(id.to_string())
        .fetch_one(pool)
        .await
        .unwrap();
        (stock, mv)
    }

    /// cancel_atomic restitui o estoque e grava um StockMovement de devolução
    /// — tudo na mesma transação (§4, §7.6).
    #[tokio::test]
    async fn cancel_atomic_restitui_estoque_e_grava_movimento() {
        let (cid, pid) = (Uuid::new_v4(), Uuid::new_v4());
        let pool = mem_pool().await;
        insert_product(&pool, cid, pid, 3.0, false).await;
        let repo = SqliteOrderRepository::new(pool.clone());
        repo.cancel_atomic(cid, Uuid::new_v4(), "cliente desistiu", &[(pid, 2.0)])
            .await
            .unwrap();
        assert_eq!(stock_and_movements(&pool, pid).await, (5.0, 1), "estoque 3+2 e 1 movimento");
    }

    /// Produto ilimitado não é restituído (não rastreia estoque) — sem
    /// movimento, sem erro.
    #[tokio::test]
    async fn cancel_atomic_pula_produto_ilimitado() {
        let (cid, pid) = (Uuid::new_v4(), Uuid::new_v4());
        let pool = mem_pool().await;
        insert_product(&pool, cid, pid, 0.0, true).await;
        let repo = SqliteOrderRepository::new(pool.clone());
        repo.cancel_atomic(cid, Uuid::new_v4(), "motivo", &[(pid, 2.0)])
            .await
            .unwrap();
        assert_eq!(stock_and_movements(&pool, pid).await, (0.0, 0), "ilimitado: sem restituição");
    }
}
