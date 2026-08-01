//! Analytics de relatórios: agregações puras sobre pedidos, produtos e
//! clientes (§1/§3 — a regra de negócio vive aqui, não na UI).
//!
//! Extraído de `desktop/src/ui/reports/sections.rs`, que calculava a DRE
//! (receita, custo, lucro, margem, ticket médio) em `f64` dentro da
//! camada de interface. Aqui o dinheiro é `Decimal` (exato) e a UI só
//! formata.
//!
//! FUSO (§6): `created_at` é gravado em UTC; todo recorte por dia/hora
//! passa por [`crate::tz`], senão o expediente das 21h em diante cai no
//! dia seguinte.
//!
//! Determinístico: nenhuma função lê o relógio — a data de referência
//! (`today`) entra por parâmetro, o que torna tudo testável.

use std::collections::HashMap;

use chrono::{Datelike, Duration, NaiveDate, Timelike};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use uuid::Uuid;

use super::model::{
    ChannelCounts, CustomerAggregate, CustomerMetrics, FinancialMetrics, MethodRevenue,
    OrdersMetrics, ProductAggregate, ProductMetrics, ReportPeriod, ReportWindow,
};
use crate::category::model::Category;
use crate::dashboard::model::DashboardPeriod;
use crate::dashboard::service::{days_in_month, period_window as dashboard_window};
use crate::money;
use crate::order::model::{DeliveryType, Order, OrderItem, OrderStatus};
use crate::product::model::Product;

/// Janela do período do relatório + a anterior equivalente.
///
/// Dia/semana/mês REAPROVEITAM a janela do dashboard (mesma regra de
/// "período anterior equivalente"); o que difere é só o FIM: o relatório
/// cobre o período inteiro (domingo, último dia do mês) enquanto o
/// dashboard vai até hoje. O ano não existe no dashboard e é montado
/// aqui.
pub fn period_window(today: NaiveDate, period: ReportPeriod) -> ReportWindow {
    if period == ReportPeriod::Yearly {
        return year_window(today);
    }
    let dash = match period {
        ReportPeriod::Daily => DashboardPeriod::Today,
        ReportPeriod::Monthly => DashboardPeriod::Month,
        _ => DashboardPeriod::Week,
    };
    let (start, prev_start, prev_end) = dashboard_window(today, dash);
    let (end, days) = match period {
        ReportPeriod::Daily => (today, 1),
        // Mês: o gráfico ocupa o mês inteiro, mas `days` conta apenas os
        // dias DECORRIDOS (1 .. hoje).
        ReportPeriod::Monthly => (
            last_of_month(start),
            ((today - start).num_days() + 1).max(1),
        ),
        _ => (start + Duration::days(6), 7),
    };
    ReportWindow { start, end, prev_start, prev_end, days }
}

/// Ano corrente (1º de janeiro → hoje) contra o ano anterior inteiro.
fn year_window(today: NaiveDate) -> ReportWindow {
    let start = NaiveDate::from_ymd_opt(today.year(), 1, 1).unwrap_or(today);
    ReportWindow {
        start,
        end: today,
        prev_start: NaiveDate::from_ymd_opt(today.year() - 1, 1, 1).unwrap_or(start),
        prev_end: NaiveDate::from_ymd_opt(today.year() - 1, 12, 31).unwrap_or(start),
        days: 365,
    }
}

/// Último dia do mês de `first`.
fn last_of_month(first: NaiveDate) -> NaiveDate {
    first
        .with_day(days_in_month(first.year(), first.month()))
        .unwrap_or(first)
}

// ── Filtros de base ──────────────────────────────────────────────────────

/// Pedidos VIVOS (sem soft delete) cuja data local cai em `from..=to`.
pub fn in_window(orders: &[Order], from: NaiveDate, to: NaiveDate) -> Vec<&Order> {
    orders
        .iter()
        .filter(|o| o.base.deleted_at.is_none())
        .filter(|o| {
            let d = crate::tz::to_local(o.base.created_at).date();
            d >= from && d <= to
        })
        .collect()
}

/// Dos pedidos da janela, os que contam como VENDA (não cancelados).
pub fn non_cancelled<'a>(orders: &[&'a Order]) -> Vec<&'a Order> {
    orders
        .iter()
        .copied()
        .filter(|o| o.status != OrderStatus::Cancelled)
        .collect()
}

// ── Financeiro (DRE) ─────────────────────────────────────────────────────

/// DRE simplificada do período: receita bruta, custo dos produtos,
/// lucro líquido, margem, ticket médio e recebimentos por forma.
pub fn financial(valid: &[&Order], products: &[Product]) -> FinancialMetrics {
    let costs = cost_index(products);
    let revenue: Decimal = valid.iter().map(|o| o.total).sum();
    let cost = products_cost(valid, &costs);
    let net = revenue - cost;
    let orders_count = valid.len();
    FinancialMetrics {
        revenue,
        cost,
        net,
        margin_pct: percent(net, revenue),
        orders_count,
        avg_ticket: divide(revenue, orders_count as u64),
        methods: method_revenue(valid),
    }
}

/// Índice `product_id → custo unitário` (apenas produtos com custo).
fn cost_index(products: &[Product]) -> HashMap<Uuid, Decimal> {
    products
        .iter()
        .filter_map(|p| p.cost_price.map(|c| (p.base.id, c)))
        .collect()
}

/// Custo dos produtos vendidos: `cost_price × quantidade`. Item sem
/// custo cadastrado entra como ZERO (não há de onde tirar o valor).
fn products_cost(orders: &[&Order], costs: &HashMap<Uuid, Decimal>) -> Decimal {
    orders
        .iter()
        .flat_map(|o| &o.items)
        .filter_map(|it| costs.get(&it.product_id).map(|c| c * money::qty(it.quantity)))
        .sum()
}

/// Receita por forma de pagamento (só as 4 formas conhecidas).
fn method_revenue(orders: &[&Order]) -> MethodRevenue {
    let mut m = MethodRevenue::default();
    for o in orders {
        match o.payment_method.as_deref() {
            Some("cash") => m.cash += o.total,
            Some("pix") => m.pix += o.total,
            Some("credit") => m.credit += o.total,
            Some("debit") => m.debit += o.total,
            _ => {}
        }
    }
    m
}

/// FIADOS em aberto: pedidos pagos pela CARTEIRA ainda não quitados.
/// Independe do período — dívida não expira.
pub fn outstanding_fiado(orders: &[Order]) -> Decimal {
    orders
        .iter()
        .filter(|o| {
            o.payment_method.as_deref() == Some("wallet")
                && !o.paid
                && o.status != OrderStatus::Cancelled
        })
        .map(|o| o.total)
        .sum()
}

// ── Pedidos ──────────────────────────────────────────────────────────────

/// KPIs e séries do sub-relatório Pedidos.
pub fn orders(all: &[&Order], valid: &[&Order], prev_valid: &[&Order]) -> OrdersMetrics {
    let total = all.len();
    let cancelled = all
        .iter()
        .filter(|o| o.status == OrderStatus::Cancelled)
        .count();
    let revenue: Decimal = valid.iter().map(|o| o.total).sum();
    OrdersMetrics {
        total,
        valid: valid.len(),
        cancelled,
        cancel_rate: percent(Decimal::from(cancelled as u64), Decimal::from(total as u64)),
        avg_ticket: divide(revenue, valid.len() as u64),
        channels: channel_counts(valid),
        by_hour: hour_counts(valid),
        prev_by_hour: hour_counts(prev_valid),
        avg_prep_minutes: avg_prep_minutes(valid),
        completed_count: valid.iter().filter(|o| is_completed(o)).count(),
    }
}

/// Canal do pedido: com forma de pagamento registrada é BALCÃO (PDV);
/// sem forma, vale o tipo de entrega escolhido no cardápio.
fn channel_counts(valid: &[&Order]) -> ChannelCounts {
    let mut c = ChannelCounts::default();
    for o in valid {
        if o.payment_method.is_some() {
            c.pdv += 1;
        } else {
            match o.delivery_type {
                DeliveryType::Delivery => c.delivery += 1,
                DeliveryType::Pickup => c.pickup += 1,
            }
        }
    }
    c
}

/// Pedidos por hora LOCAL (0..23).
fn hour_counts(orders: &[&Order]) -> [u32; 24] {
    let mut totals = [0_u32; 24];
    for o in orders {
        let h = crate::tz::to_local(o.base.created_at).hour() as usize;
        if let Some(slot) = totals.get_mut(h) {
            *slot += 1;
        }
    }
    totals
}

/// Pedido que chegou ao fim do preparo — base do tempo médio.
fn is_completed(o: &Order) -> bool {
    matches!(o.status, OrderStatus::Ready | OrderStatus::Delivered)
}

/// Tempo médio de preparo em MINUTOS (`created_at` → `updated_at`).
///
/// Descarta outliers fora de 5s..6h: abaixo disso é lançamento em lote
/// (não houve preparo) e acima é pedido esquecido em aberto.
fn avg_prep_minutes(orders: &[&Order]) -> Option<f64> {
    let mut sum = 0.0_f64;
    let mut count = 0_u32;
    for o in orders.iter().filter(|o| is_completed(o)) {
        let delta = (o.base.updated_at - o.base.created_at).num_seconds();
        if !(5..=6 * 3600).contains(&delta) {
            continue;
        }
        sum += delta as f64;
        count += 1;
    }
    (count > 0).then(|| sum / count as f64 / 60.0)
}

// ── Produtos ─────────────────────────────────────────────────────────────

/// Agregação por produto: quantidade, receita rateada e custo.
///
/// A receita do pedido é RATEADA entre os itens pela participação de
/// cada linha no subtotal — assim descontos e acréscimos do pedido
/// aparecem distribuídos, e a soma das receitas bate com o faturamento.
pub fn products(
    valid: &[&Order],
    catalog: &[Product],
    categories: &[Category],
) -> ProductMetrics {
    let by_id: HashMap<Uuid, &Product> = catalog.iter().map(|p| (p.base.id, p)).collect();
    let cat_by_id: HashMap<Uuid, &Category> = categories.iter().map(|c| (c.base.id, c)).collect();
    let mut agg: HashMap<Uuid, ProductAggregate> = HashMap::new();

    for o in valid {
        let subtotal: Decimal = o
            .items
            .iter()
            .map(|i| i.unit_price * money::qty(i.quantity))
            .sum();
        for it in &o.items {
            let entry = agg
                .entry(it.product_id)
                .or_insert_with(|| empty_aggregate(it, &by_id, &cat_by_id));
            entry.quantity += it.quantity;
            let line = it.unit_price * money::qty(it.quantity);
            // Rateio: (total × linha) ÷ subtotal. Multiplicar antes de
            // dividir mantém mais precisão que a fração isolada.
            if subtotal > Decimal::ZERO {
                entry.revenue += (o.total * line) / subtotal;
            }
            if let Some(c) = by_id.get(&it.product_id).and_then(|p| p.cost_price) {
                entry.cost += c * money::qty(it.quantity);
            }
        }
    }
    build_product_metrics(agg)
}

/// Agregado zerado de um item: nome e categoria resolvidos do catálogo,
/// com fallback no snapshot gravado no próprio item.
fn empty_aggregate(
    it: &OrderItem,
    by_id: &HashMap<Uuid, &Product>,
    cat_by_id: &HashMap<Uuid, &Category>,
) -> ProductAggregate {
    let product = by_id.get(&it.product_id);
    ProductAggregate {
        product_id: it.product_id,
        name: product
            .map(|p| p.name.clone())
            .unwrap_or_else(|| it.product_name.clone()),
        category_name: product
            .and_then(|p| p.category_id)
            .and_then(|cid| cat_by_id.get(&cid))
            .map(|c| c.name.clone())
            .unwrap_or_default(),
        quantity: 0.0,
        revenue: Decimal::ZERO,
        cost: Decimal::ZERO,
    }
}

/// Totais, margem ponderada e rankings a partir dos agregados.
/// Empates desempatam pelo NOME: sem isso a ordem dependia do
/// `HashMap` e o "mais vendido" alternava a cada refresh.
fn build_product_metrics(agg: HashMap<Uuid, ProductAggregate>) -> ProductMetrics {
    let mut ranking: Vec<ProductAggregate> = agg.into_values().collect();
    let total_units: f64 = ranking.iter().map(|a| a.quantity).sum();
    let total_revenue: Decimal = ranking.iter().map(|a| a.revenue).sum();
    let total_cost: Decimal = ranking.iter().map(|a| a.cost).sum();

    let top_by_quantity = ranking
        .iter()
        .max_by(|a, b| {
            a.quantity
                .partial_cmp(&b.quantity)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.name.cmp(&a.name))
        })
        .cloned();
    ranking.sort_by(|a, b| b.revenue.cmp(&a.revenue).then_with(|| a.name.cmp(&b.name)));

    ProductMetrics {
        total_units,
        total_revenue,
        total_cost,
        margin_pct: percent(total_revenue - total_cost, total_revenue),
        top_by_revenue: ranking.first().cloned(),
        top_by_quantity,
        ranking,
    }
}

// ── Clientes ─────────────────────────────────────────────────────────────

/// KPIs e ranking do sub-relatório Clientes.
///
/// `valid` são os pedidos válidos do período (define quem está ATIVO) e
/// `all` é o histórico completo (define a primeira compra e o LTV).
pub fn customers(
    valid: &[&Order],
    all: &[Order],
    start: NaiveDate,
    end: NaiveDate,
) -> CustomerMetrics {
    let first_order = first_order_dates(all);
    let ltv = lifetime_value(all);

    let mut active: Vec<Uuid> = valid
        .iter()
        .filter(|o| !o.customer_id.is_nil())
        .map(|o| o.customer_id)
        .collect();
    active.sort_unstable();
    active.dedup();

    let (mut new_count, mut returning_count) = (0_i32, 0_i32);
    for cid in &active {
        match first_order.get(cid) {
            Some(d) if *d >= start && *d <= end => new_count += 1,
            Some(_) => returning_count += 1,
            None => {}
        }
    }

    let avg_ltv = divide(ltv.values().map(|(r, _)| *r).sum(), ltv.len() as u64);
    CustomerMetrics {
        active_count: active.len(),
        new_count,
        returning_count,
        return_rate: percent(
            Decimal::from(returning_count),
            Decimal::from(active.len() as u64),
        ),
        avg_ltv,
        ranking: customer_ranking(ltv, avg_ltv),
    }
}

/// Data (local) da PRIMEIRA compra de cada cliente identificado.
fn first_order_dates(all: &[Order]) -> HashMap<Uuid, NaiveDate> {
    let mut first: HashMap<Uuid, NaiveDate> = HashMap::new();
    for o in all.iter().filter(|o| counts_for_history(o)) {
        let d = crate::tz::to_local(o.base.created_at).date();
        first
            .entry(o.customer_id)
            .and_modify(|cur| {
                if d < *cur {
                    *cur = d;
                }
            })
            .or_insert(d);
    }
    first
}

/// Receita acumulada e nº de pedidos por cliente (histórico completo).
fn lifetime_value(all: &[Order]) -> HashMap<Uuid, (Decimal, i64)> {
    let mut ltv: HashMap<Uuid, (Decimal, i64)> = HashMap::new();
    for o in all.iter().filter(|o| counts_for_history(o)) {
        let e = ltv.entry(o.customer_id).or_insert((Decimal::ZERO, 0));
        e.0 += o.total;
        e.1 += 1;
    }
    ltv
}

/// Pedido que entra no histórico do cliente: vivo, não cancelado e com
/// cliente identificado (pedido de balcão anônimo não tem LTV).
fn counts_for_history(o: &Order) -> bool {
    o.base.deleted_at.is_none() && o.status != OrderStatus::Cancelled && !o.customer_id.is_nil()
}

/// Ranking por LTV (desc); empate → `customer_id` (determinístico).
fn customer_ranking(
    ltv: HashMap<Uuid, (Decimal, i64)>,
    avg_ltv: Decimal,
) -> Vec<CustomerAggregate> {
    let mut ranking: Vec<CustomerAggregate> = ltv
        .into_iter()
        .map(|(customer_id, (revenue, orders))| CustomerAggregate {
            customer_id,
            revenue,
            orders,
            is_vip: revenue >= avg_ltv * dec!(2),
        })
        .collect();
    ranking.sort_by(|a, b| {
        b.revenue
            .cmp(&a.revenue)
            .then_with(|| a.customer_id.cmp(&b.customer_id))
    });
    ranking
}

// ── Utilitários ──────────────────────────────────────────────────────────

/// `part ÷ whole × 100`, protegido contra divisão por zero.
fn percent(part: Decimal, whole: Decimal) -> Decimal {
    if whole > Decimal::ZERO {
        part / whole * dec!(100)
    } else {
        Decimal::ZERO
    }
}

/// Média protegida contra divisão por zero (ticket médio, LTV médio).
fn divide(total: Decimal, count: u64) -> Decimal {
    if count > 0 {
        total / Decimal::from(count)
    } else {
        Decimal::ZERO
    }
}
