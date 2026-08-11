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

/// Baixa/restitui os INSUMOS da ficha técnica de um produto na mesma
/// transação do pedido (venda/edição/cancelamento). `product_delta` segue o
/// sinal do delta do produto. Não bloqueia (insumo pode ficar negativo).
/// Bumpa `insumos.updated_at` para os desktops puxarem o novo estoque; o
/// movimento nasce `synced = true` (o servidor é o hub). Em `cancel`, id
/// determinístico evita restituir em dobro entre terminais.
#[allow(clippy::too_many_arguments)]
async fn consume_recipe(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    order_id: Uuid,
    product_id: Uuid,
    product_delta: f64,
    reason: &str,
    deterministic: bool,
    now: chrono::NaiveDateTime,
) -> Result<(), CoreError> {
    let recipe: Vec<(Uuid, f64)> = sqlx::query_as(
        "SELECT insumo_id, quantity FROM product_ingredients
         WHERE company_id = $1 AND product_id = $2 ORDER BY sort_order ASC",
    )
    .bind(company_id)
    .bind(product_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(map_db)?;
    for (insumo_id, qty_per_unit) in recipe {
        let delta = qty_per_unit * product_delta;
        if delta.abs() < f64::EPSILON { continue; }
        sqlx::query(
            "UPDATE insumos SET stock_quantity = stock_quantity + $1, updated_at = $2, synced = false
              WHERE company_id = $3 AND id = $4 AND deleted_at IS NULL",
        )
        .bind(delta)
        .bind(now)
        .bind(company_id)
        .bind(insumo_id)
        .execute(&mut **tx)
        .await
        .map_err(map_db)?;
        let mv_id = if deterministic {
            letaf_core::deterministic_id::insumo_movement_once(order_id, reason, product_id, insumo_id)
        } else {
            Uuid::new_v4()
        };
        sqlx::query(
            "INSERT INTO insumo_movements
                (id, company_id, insumo_id, delta, reason, order_id, created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $7, NULL, true)
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(mv_id)
        .bind(company_id)
        .bind(insumo_id)
        .bind(delta)
        .bind(reason)
        .bind(order_id)
        .bind(now)
        .execute(&mut **tx)
        .await
        .map_err(map_db)?;
    }
    Ok(())
}

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
    paid: bool,
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
            paid: r.paid,
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
    list_unit_price: Option<Decimal>,
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
            list_unit_price: r.list_unit_price,
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
        coupon: Option<&letaf_core::order::repository::CouponLimits>,
    ) -> Result<(), CoreError> {
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        let now = chrono::Utc::now().naive_utc();

        // 0) Cupom: serializa resgates CONCORRENTES do mesmo cupom nesta empresa
        // (§11 — fecha o TOCTOU do limite). O advisory lock é de TRANSAÇÃO
        // (liberado no commit/rollback): o 2º pedido espera o 1º commitar e só
        // então reconta (passo 3), enxergando o uso dele. Chave = hash de
        // "company:code"; colisão de hash só serializa cupons distintos à toa
        // (sem impacto de correção).
        if let Some(c) = coupon {
            sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
                .bind(format!("coupon:{}:{}", order.base.company_id, c.code))
                .execute(&mut *tx)
                .await
                .map_err(map_db)?;
        }

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
                    // A venda nasce em UM terminal só (o pedido é criado lá),
                    // então não há evento duplicado a deduplicar.
                    None,
                    now,
                )
                .await?;
                consume_recipe(&mut tx, order.base.company_id, order.base.id, *product_id, -*qty, "sale", false, now).await?;
            }
        }

        // 2) Insere o pedido + itens (mesma tx).
        sqlx::query(
            "INSERT INTO orders (id, company_id, customer_id, number, status, total, delivery_type, notes, cancellation_reason, created_at, updated_at, deleted_at, synced, coupon_code, discount_amount, additional_amount, payment_method, paid)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)",
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
        .bind(order.paid)
        .execute(&mut *tx)
        .await
        .map_err(map_db)?;

        for item in &order.items {
            sqlx::query(
                "INSERT INTO order_items (id, company_id, order_id, product_id, product_name, quantity, unit_price, subtotal, notes, addons_json, created_at, updated_at, deleted_at, synced, list_unit_price)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)",
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
            .bind(item.list_unit_price)
            .execute(&mut *tx)
            .await
            .map_err(map_db)?;
        }

        // 3) Enforcement race-safe do cupom: reconta o uso JÁ incluindo este
        // pedido (mesma tx). As predicates espelham `count_*` (status<>cancelled,
        // deleted_at IS NULL). `>` (não `>=`) porque a contagem inclui o pedido
        // atual. Estourou → `Err` → a tx inteira reverte (pedido e baixa de
        // estoque desfeitos). O advisory lock (passo 0) garante que concorrentes
        // não escapem entre a contagem e o commit.
        if let Some(c) = coupon {
            let company = order.base.company_id;
            let cust = opt_customer(order.customer_id);
            if c.usage_limit > 0 {
                let (n,): (i64,) = sqlx::query_as(
                    "SELECT COUNT(*) FROM orders
                     WHERE company_id = $1 AND UPPER(coupon_code) = UPPER($2)
                       AND status <> 'cancelled' AND deleted_at IS NULL",
                )
                .bind(company).bind(&c.code)
                .fetch_one(&mut *tx).await.map_err(map_db)?;
                if n > c.usage_limit as i64 {
                    return Err(CoreError::Validation("Cupom esgotado".into()));
                }
            }
            if c.per_user_limit > 0 {
                let (n,): (i64,) = sqlx::query_as(
                    "SELECT COUNT(*) FROM orders
                     WHERE company_id = $1 AND customer_id = $2
                       AND UPPER(coupon_code) = UPPER($3)
                       AND status <> 'cancelled' AND deleted_at IS NULL",
                )
                .bind(company).bind(cust).bind(&c.code)
                .fetch_one(&mut *tx).await.map_err(map_db)?;
                if n > c.per_user_limit as i64 {
                    return Err(CoreError::Validation(
                        "Você já atingiu o limite de uso deste cupom".into(),
                    ));
                }
            }
            if c.first_purchase {
                // Inclui este pedido → `> 1` significa que já havia pedido
                // anterior (espelha `customer_prior_orders > 0` do `evaluate`).
                let (n,): (i64,) = sqlx::query_as(
                    "SELECT COUNT(*) FROM orders
                     WHERE company_id = $1 AND customer_id = $2
                       AND status <> 'cancelled' AND deleted_at IS NULL",
                )
                .bind(company).bind(cust)
                .fetch_one(&mut *tx).await.map_err(map_db)?;
                if n > 1 {
                    return Err(CoreError::Validation(
                        "Cupom válido apenas na primeira compra".into(),
                    ));
                }
            }
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

    async fn find_board(
        &self,
        company_id: Uuid,
        entregues_desde: chrono::NaiveDate,
    ) -> Result<Vec<Order>, CoreError> {
        let rows = sqlx::query_as::<_, OrderRow>(
            "SELECT * FROM orders
             WHERE company_id = $1 AND deleted_at IS NULL
               AND (status NOT IN ('delivered', 'cancelled') OR created_at >= $2)
             ORDER BY created_at DESC",
        )
        .bind(company_id)
        .bind(entregues_desde.and_hms_opt(0, 0, 0).unwrap_or_default())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        let mut orders: Vec<Order> = rows.into_iter().map(Order::from).collect();
        self.attach_items(&mut orders).await?;
        Ok(orders)
    }

    async fn search(
        &self,
        company_id: Uuid,
        termo: &str,
        limite: i64,
        deslocamento: i64,
    ) -> Result<Vec<Order>, CoreError> {
        let curinga = format!("%{}%", termo.to_lowercase());
        let prefixo = format!("{termo}%");
        let rows = sqlx::query_as::<_, OrderRow>(
            "SELECT o.* FROM orders o
             LEFT JOIN customers c ON c.id = o.customer_id
             WHERE o.company_id = $1 AND o.deleted_at IS NULL
               AND (o.number::text LIKE $2
                    OR LOWER(c.name) LIKE $3
                    OR c.phone LIKE $3)
             ORDER BY o.created_at DESC
             LIMIT $4 OFFSET $5",
        )
        .bind(company_id)
        .bind(&prefixo)
        .bind(&curinga)
        .bind(limite)
        .bind(deslocamento)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        let mut orders: Vec<Order> = rows.into_iter().map(Order::from).collect();
        self.attach_items(&mut orders).await?;
        Ok(orders)
    }

    async fn find_all_light(&self, company_id: Uuid) -> Result<Vec<Order>, CoreError> {
        let rows = sqlx::query_as::<_, OrderRow>(
            "SELECT * FROM orders WHERE company_id = $1 AND deleted_at IS NULL
             ORDER BY created_at DESC",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(Order::from).collect())
    }

    async fn find_in_period(
        &self,
        company_id: Uuid,
        inicio: chrono::NaiveDate,
        fim: chrono::NaiveDate,
    ) -> Result<Vec<Order>, CoreError> {
        let rows = sqlx::query_as::<_, OrderRow>(
            "SELECT * FROM orders
             WHERE company_id = $1 AND deleted_at IS NULL
               AND created_at >= $2 AND created_at < $3
             ORDER BY created_at DESC",
        )
        .bind(company_id)
        .bind(inicio.and_hms_opt(0, 0, 0).unwrap_or_default())
        .bind(fim.succ_opt().unwrap_or(fim).and_hms_opt(0, 0, 0).unwrap_or_default())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        let mut orders: Vec<Order> = rows.into_iter().map(Order::from).collect();
        self.attach_items(&mut orders).await?;
        Ok(orders)
    }

    async fn find_unpaid_wallet(&self, company_id: Uuid) -> Result<Vec<Order>, CoreError> {
        let rows = sqlx::query_as::<_, OrderRow>(
            "SELECT * FROM orders
             WHERE company_id = $1 AND deleted_at IS NULL
               AND payment_method = 'wallet' AND paid = false AND status <> 'cancelled'
             ORDER BY created_at DESC",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        let mut orders: Vec<Order> = rows.into_iter().map(Order::from).collect();
        self.attach_items(&mut orders).await?;
        Ok(orders)
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

    async fn update_payment(
        &self,
        company_id: Uuid,
        id: Uuid,
        payment_method: Option<&str>,
        paid: bool,
    ) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        sqlx::query(
            "UPDATE orders SET payment_method = $1, paid = $2, updated_at = $3, synced = false
             WHERE company_id = $4 AND id = $5 AND deleted_at IS NULL",
        )
        .bind(payment_method)
        .bind(paid)
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
                insert_stock_movement(&mut tx, order.base.company_id, *product_id, *delta, "edit", Some(order.base.id), None, now)
                    .await?;
                consume_recipe(&mut tx, order.base.company_id, order.base.id, *product_id, *delta, "edit", false, now).await?;
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
                "INSERT INTO order_items (id, company_id, order_id, product_id, product_name, quantity, unit_price, subtotal, notes, addons_json, created_at, updated_at, deleted_at, synced, list_unit_price)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)",
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
            .bind(item.list_unit_price)
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
                insert_stock_movement(
                    &mut tx,
                    company_id,
                    *product_id,
                    *qty,
                    "cancel",
                    Some(id),
                    Some(letaf_core::deterministic_id::stock_movement_once(id, "cancel", *product_id)),
                    now,
                )
                    .await?;
                consume_recipe(&mut tx, company_id, id, *product_id, *qty, "cancel", true, now).await?;
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

    async fn sync_upsert(&self, order: &Order) -> Result<bool, CoreError> {
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
            reconcile_order_stock(&mut tx, order).await?;
        }
        tx.commit().await.map_err(map_db)?;
        Ok(incoming_wins)
    }
}

/// Upsert da linha de `Order` aplicando last-write-wins por `updated_at` (§7.7).
async fn upsert_order_row(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    order: &Order,
) -> Result<(), CoreError> {
    sqlx::query(
        "INSERT INTO orders (id, company_id, customer_id, number, status, total, delivery_type, notes, cancellation_reason, created_at, updated_at, deleted_at, synced, coupon_code, discount_amount, additional_amount, payment_method, paid)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
         ON CONFLICT (id) DO UPDATE SET
                 -- Vincular o cliente a um pedido de balcao ja criado
                 -- (fidelidade/carteira) mudava `updated_at` e vencia o
                 -- LWW, mas o `customer_id` ficava fora do UPDATE: o
                 -- pedido continuava anonimo do outro lado.
                 -- COALESCE: o desktop manda o UUID nil quando não há
                 -- cliente, que vira NULL. Sem isso, uma cópia defasada
                 -- que só mudou o status APAGAVA o vínculo feito na web.
                 customer_id = COALESCE(EXCLUDED.customer_id, orders.customer_id),
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
             payment_method = EXCLUDED.payment_method,
             paid = EXCLUDED.paid
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
    .bind(order.paid)
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
        "INSERT INTO order_items (id, company_id, order_id, product_id, product_name, quantity, unit_price, subtotal, notes, addons_json, created_at, updated_at, deleted_at, synced, list_unit_price)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
         ON CONFLICT (id) DO UPDATE SET
             quantity = EXCLUDED.quantity,
             unit_price = EXCLUDED.unit_price,
             subtotal = EXCLUDED.subtotal,
             notes = EXCLUDED.notes,
             addons_json = EXCLUDED.addons_json,
             list_unit_price = EXCLUDED.list_unit_price,
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
    .bind(item.list_unit_price)
    .execute(&mut **tx)
    .await
    .map_err(map_db)?;
    Ok(())
}

/// Reconcilia o LEDGER de estoque com o pedido que venceu o LWW.
///
/// O delta da edição era calculado por cada terminal contra a cópia LOCAL do
/// pedido, mas só uma versão sobrevive ao last-write-wins. Com A editando
/// 2→3 (delta −1) e B editando 2→5 (delta −3) na mesma janela, os DOIS deltas
/// eram aplicados (−4) enquanto o pedido final dizia 5: estoque e pedido
/// ficavam permanentemente descasados, convergentes e errados, sem erro
/// nenhum. O mesmo valia ao contrário, sobrando estoque fantasma.
///
/// Aqui o SERVIDOR — que é a autoridade (§11) — impõe o invariante: a soma
/// dos deltas do pedido para cada produto tem que valer exatamente
/// `-quantidade` do pedido vencedor (ou `0` se ele foi cancelado, porque o
/// estoque volta). Emite um movimento COMPENSATÓRIO com a diferença; quando
/// já está batendo, a diferença é zero e nada é gravado — idempotente.
///
/// Só mexe em produto que JÁ tem movimento deste pedido: pedido histórico sem
/// controle de estoque (ou produto ilimitado) continua intocado, senão um
/// push antigo geraria uma baixa retroativa que nunca existiu.
async fn reconcile_order_stock(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    order: &Order,
) -> Result<(), CoreError> {
    let cancelado = order.status == OrderStatus::Cancelled || order.base.deleted_at.is_some();

    // Soma dos deltas já aplicados por este pedido, por produto.
    let aplicados: Vec<(Uuid, f64)> = sqlx::query_as(
        "SELECT product_id, COALESCE(SUM(delta), 0)::float8
           FROM stock_movements
          WHERE company_id = $1 AND order_id = $2
          GROUP BY product_id",
    )
    .bind(order.base.company_id)
    .bind(order.base.id)
    .fetch_all(&mut **tx)
    .await
    .map_err(map_db)?;

    for (product_id, soma) in aplicados {
        // Produto ilimitado não tem estoque a reconciliar.
        let ilimitado: Option<bool> = sqlx::query_scalar(
            "SELECT unlimited_stock FROM products WHERE company_id = $1 AND id = $2",
        )
        .bind(order.base.company_id)
        .bind(product_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(map_db)?;
        if ilimitado.unwrap_or(true) {
            continue;
        }

        let quantidade: f64 = if cancelado {
            0.0
        } else {
            order
                .items
                .iter()
                .filter(|i| i.product_id == product_id)
                .map(|i| i.quantity)
                .sum()
        };
        let alvo = -quantidade;
        let compensacao = alvo - soma;
        // Tolerância de meia unidade de milésimo: `quantity` é f64 (venda por
        // peso), e comparar por igualdade exata geraria movimentos de ruído.
        if compensacao.abs() < 0.0005 {
            continue;
        }
        tracing::info!(
            "Pedido {}: ledger de estoque do produto {} valia {soma}, alvo {alvo} \
             — compensando {compensacao}",
            order.base.id,
            product_id
        );
        let agora = chrono::Utc::now().naive_utc();
        insert_stock_movement(
            tx,
            order.base.company_id,
            product_id,
            compensacao,
            "edit_reconcile",
            Some(order.base.id),
            None,
            agora,
        )
        .await?;
        // Aplica ao materializado e bumpa `updated_at` para a correção descer
        // aos terminais pelo cursor de pull (mesma regra do `apply`).
        sqlx::query(
            "UPDATE products
                SET stock_quantity = stock_quantity + $1,
                    updated_at = GREATEST(products.updated_at, (now() AT TIME ZONE 'utc'))
              WHERE company_id = $2 AND id = $3 AND unlimited_stock = false",
        )
        .bind(compensacao)
        .bind(order.base.company_id)
        .bind(product_id)
        .execute(&mut **tx)
        .await
        .map_err(map_db)?;
    }
    Ok(())
}
