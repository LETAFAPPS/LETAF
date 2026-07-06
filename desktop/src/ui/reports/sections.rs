use std::collections::HashMap;

use chrono::{Duration, NaiveDate, Timelike};
use slint::{Color, SharedString};
use uuid::Uuid;

use letaf_core::category::model::Category;
use letaf_core::customer::model::Customer;
use letaf_core::order::model::{DeliveryType, Order, OrderStatus};
use letaf_core::product::model::Product;

use rust_decimal::prelude::ToPrimitive;

fn money_br(v: f64) -> String {
    crate::format::money_br(letaf_core::money::from_db_f64(v))
}
use crate::{
    ReportHBar, ReportHourlyBar, ReportNewVsReturning,
};

use super::state::Granularity;
use super::snapshot::{Snapshot, TopCustomerRaw, TopProductRaw};
use super::super::helpers::half_donut_arc;
use super::helpers::{
    avg_prep_sub, avg_prep_value, build_daily, color_for, dre, kpi, money_plain,
};

// ── Builders por sub-relatório ───────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub(crate) fn fill_financial(
    snap: &mut Snapshot,
    in_window: &[&Order],
    valid: &[&Order],
    product_by_id: &HashMap<Uuid, &Product>,
    start: NaiveDate,
    end: NaiveDate,
    period_days: i64,
    today: NaiveDate,
    granularity: Granularity,
) {
    let revenue: f64 = valid.iter().map(|o| o.total.to_f64().unwrap_or(0.0)).sum();
    let cost: f64 = valid
        .iter()
        .flat_map(|o| &o.items)
        .map(|it| {
            product_by_id
                .get(&it.product_id)
                .and_then(|p| p.cost_price)
                .map(|c| c.to_f64().unwrap_or(0.0) * it.quantity)
                .unwrap_or(0.0)
        })
        .sum();
    let net = revenue - cost;
    let orders_count = valid.len();
    let avg_ticket = if orders_count > 0 { revenue / orders_count as f64 } else { 0.0 };

    // Comparativo período anterior
    let prev_start = start - Duration::days(period_days);
    let prev_end = start - Duration::days(1);
    let prev_revenue: f64 = in_window // ← in_window é só do período atual; preciso filtrar `all` orders.
        .iter()
        .filter(|_| false) // placeholder: usaremos comparação simples sem prev abaixo
.map(|o| o.total.to_f64().unwrap_or(0.0))
        .sum();
    let _ = (prev_start, prev_end, prev_revenue);

    // KPIs
    snap.kpis = vec![
        kpi(
            "RECEITA BRUTA",
            &money_br(revenue),
            &format!("{} Pedidos · {} Dias", orders_count, period_days),
            Color::from_rgb_u8(0x2E, 0x7D, 0x32),
            "neutral",
            "atividade",
            false,
        ),
        kpi(
            "CUSTOS",
            &money_br(cost),
            "Custo Produtos",
            Color::from_rgb_u8(0xE5, 0x39, 0x35),
            "neutral",
            "saida-estoque",
            false,
        ),
        kpi(
            "LUCRO LÍQUIDO",
            &money_br(net),
            &format!(
                "Margem {:.0}%",
                if revenue > 0.0 { net / revenue * 100.0 } else { 0.0 }
            ),
            Color::from_rgb_u8(0x43, 0xA0, 0x47),
            "neutral",
            "pay-carteira",
            false,
        ),
        kpi(
            "TICKET MÉDIO",
            &money_br(avg_ticket),
            &format!("{} Pedidos no período", orders_count),
            Color::from_rgb_u8(0x2E, 0x7D, 0x32),
            "neutral",
            "coupons",
            true,
        ),
    ];

    // Receita diária (gráfico) — tooltip sem prefixo "R$ " (estava
    // cortando dentro da pílula do candle).
    snap.daily_bars = build_daily(start, end, today, valid, granularity,
        |o| o.total.to_f64().unwrap_or(0.0), money_plain,
        Color::from_rgb_u8(0x66, 0xBB, 0x6A));

    // DRE simplificada — somente linhas com dados disponíveis no
    // domínio. Despesas/Taxas/Impostos serão adicionadas quando
    // entrarem como entidades persistidas.
    snap.dre_lines = vec![
        dre("Receita Bruta", &format!("+{}", money_br(revenue)), "pos"),
        dre("Custo de Produtos", &format!("−{}", money_br(cost)), "neg"),
        dre("LUCRO LÍQUIDO", &money_br(net), "total"),
    ];

    // Recebimentos por método
    let mut method_sum: HashMap<String, f64> = HashMap::new();
    for o in valid {
        let k = o.payment_method.clone().unwrap_or_else(|| "outros".into());
        *method_sum.entry(k).or_default() += o.total.to_f64().unwrap_or(0.0);
    }
    struct MethodDef {
        key: &'static str,
        label: &'static str,
        icon: &'static str,
        color: Color,
    }
    // Ordem solicitada: Dinheiro, PIX, Cartão Crédito, Cartão Débito.
    // Cores iguais ao Dashboard: Dinheiro verde, PIX azul, Crédito
    // vermelho, Débito amarelo.
    let palette = [
        MethodDef { key: "cash", label: "Dinheiro", icon: "pay-dinheiro", color: Color::from_rgb_u8(0x2E, 0x7D, 0x32) },
        MethodDef { key: "pix", label: "PIX", icon: "pay-pix", color: Color::from_rgb_u8(0x1E, 0x88, 0xE5) },
        MethodDef { key: "credit", label: "Cartão Crédito", icon: "pay-cartao-credito", color: Color::from_rgb_u8(0xE5, 0x39, 0x35) },
        MethodDef { key: "debit", label: "Cartão Débito", icon: "pay-cartao-debito", color: Color::from_rgb_u8(0xF9, 0xA8, 0x25) },
    ];
    // Total = soma APENAS das formas conhecidas (gauge 100% preenchido,
    // como na tela de Caixa) — pedidos sem método ("outros") ficam fora.
    let total_method: f64 = palette
        .iter()
        .map(|m| method_sum.get(m.key).copied().unwrap_or(0.0))
        .sum();
    snap.method_total = money_br(total_method);
    let mut method_bars = Vec::new();
    // Acumulador de fração para encadear os arcos da meia-lua (gauge).
    let mut acc = 0.0_f64;
    for m in &palette {
        let v = method_sum.get(m.key).copied().unwrap_or(0.0);
        if v > 0.0 || method_sum.is_empty() {
            let frac = if total_method > 0.0 { v / total_method } else { 0.0 };
            let arc = half_donut_arc(acc, acc + frac);
            acc += frac;
            method_bars.push(ReportHBar {
                label: SharedString::from(m.label),
                value_display: SharedString::from(money_br(v)),
                progress: frac as f32,
                bar_color: m.color,
                icon_key: SharedString::from(m.icon),
                arc_commands: SharedString::from(arc),
            });
        }
    }
    snap.method_bars = method_bars;
}

pub(crate) fn fill_orders(
    snap: &mut Snapshot,
    in_window: &[&Order],
    valid: &[&Order],
    start: NaiveDate,
    end: NaiveDate,
    today: NaiveDate,
    granularity: Granularity,
) {
    let total = in_window.len();
    let cancel = in_window
        .iter()
        .filter(|o| o.status == OrderStatus::Cancelled)
        .count();
    let cancel_rate = if total > 0 { (cancel as f64 / total as f64) * 100.0 } else { 0.0 };
    let avg_ticket = if !valid.is_empty() {
        valid.iter().map(|o| o.total.to_f64().unwrap_or(0.0)).sum::<f64>() / valid.len() as f64
    } else {
        0.0
    };
    snap.kpis = vec![
        kpi(
            "TOTAL DE PEDIDOS",
            &total.to_string(),
            &format!("{} Válidos · {} Cancelados", valid.len(), cancel),
            Color::from_rgb_u8(0xE6, 0x51, 0x00),
            "neutral",
            "orders",
            true,
        ),
        kpi(
            "TICKET MÉDIO",
            &money_br(avg_ticket),
            "Receita por pedido",
            Color::from_rgb_u8(0x2E, 0x7D, 0x32),
            "neutral",
            "coupons",
            false,
        ),
        kpi(
            "TAXA CANCELAMENTO",
            &format!("{:.1}%", cancel_rate),
            &format!("{} Cancelados", cancel),
            Color::from_rgb_u8(0xE5, 0x39, 0x35),
            "neutral",
            "nao-conformidade",
            false,
        ),
        kpi(
            "TEMPO MÉDIO PREPARO",
            &avg_prep_value(valid),
            &avg_prep_sub(valid),
            Color::from_rgb_u8(0x1E, 0x88, 0xE5),
            "neutral",
            "relogio",
            true,
        ),
    ];

    // Pedidos por dia
    snap.orders_bars = build_daily(
        start, end, today, in_window, granularity,
        |_| 1.0,
        |v| format!("{}", v.round() as i64),
        Color::from_rgb_u8(0xFB, 0x8C, 0x00),
    );

    // Por canal
    let mut delivery = 0;
    let mut pickup = 0;
    let mut pdv = 0;
    for o in valid {
        if o.payment_method.is_some() {
            pdv += 1;
        } else {
            match o.delivery_type {
                DeliveryType::Delivery => delivery += 1,
                DeliveryType::Pickup => pickup += 1,
            }
        }
    }
    let max_ch = [delivery, pickup, pdv].iter().copied().max().unwrap_or(0).max(1) as f64;
    // Ordem solicitada: Balcão, Entrega, Retirada.
    // Cores: Balcão = verde, Entrega = azul, Retirada = laranja
    // (Balcão e Retirada trocadas pelo usuário).
    snap.channel_bars = vec![
        ReportHBar {
            label: SharedString::from("Balcão"),
            value_display: SharedString::from(pdv.to_string()),
            progress: (pdv as f64 / max_ch) as f32,
            bar_color: Color::from_rgb_u8(0x2E, 0x7D, 0x32),
            icon_key: SharedString::from("pdv"),
            arc_commands: SharedString::new(),
        },
        ReportHBar {
            label: SharedString::from("Entrega"),
            value_display: SharedString::from(delivery.to_string()),
            progress: (delivery as f64 / max_ch) as f32,
            bar_color: Color::from_rgb_u8(0x1E, 0x88, 0xE5),
            icon_key: SharedString::from("rastreamento"),
            arc_commands: SharedString::new(),
        },
        ReportHBar {
            label: SharedString::from("Retirada"),
            value_display: SharedString::from(pickup.to_string()),
            progress: (pickup as f64 / max_ch) as f32,
            bar_color: Color::from_rgb_u8(0xE6, 0x51, 0x00),
            icon_key: SharedString::from("recebimento"),
            arc_commands: SharedString::new(),
        },
    ];

    // Pedidos por horário (08h..22h)
    let mut by_hour = [0u32; 24];
    for o in valid {
        let h = o.base.created_at.hour() as usize;
        if h < 24 { by_hour[h] += 1; }
    }
    let max_h = by_hour.iter().copied().max().unwrap_or(0).max(1) as f64;
    let mut hourly = Vec::with_capacity(15);
    for (h, count) in by_hour.iter().enumerate().skip(8).take(15) {
        let label = if h % 2 == 0 { format!("{:02}h", h) } else { String::new() };
        hourly.push(ReportHourlyBar {
            label: SharedString::from(label),
            progress: (*count as f64 / max_h) as f32,
            value_display: SharedString::from(if *count > 0 { count.to_string() } else { String::new() }),
        });
    }
    snap.hourly_bars = hourly;
}

pub(crate) fn fill_products(
    snap: &mut Snapshot,
    valid: &[&Order],
    product_by_id: &HashMap<Uuid, &Product>,
    category_by_id: &HashMap<Uuid, &Category>,
) {
    // Agrega por produto.
    struct Agg { qty: f64, revenue: f64, cost: f64, name: String, category: String, swatch: Color }
    let mut by_pid: HashMap<Uuid, Agg> = HashMap::new();
    for o in valid {
        let subtotal_order: f64 = o.items.iter().map(|i| i.unit_price.to_f64().unwrap_or(0.0) * i.quantity).sum();
        for it in &o.items {
            let share = if subtotal_order > 0.0 {
(it.unit_price.to_f64().unwrap_or(0.0) * it.quantity) / subtotal_order
            } else {
                0.0
            };
            let entry = by_pid.entry(it.product_id).or_insert_with(|| {
                let p = product_by_id.get(&it.product_id);
                let name = p.map(|p| p.name.clone()).unwrap_or_else(|| it.product_name.clone());
                let cat_name = p
                    .and_then(|p| p.category_id)
                    .and_then(|cid| category_by_id.get(&cid))
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| "".into());
                let swatch = color_for(&cat_name);
                Agg { qty: 0.0, revenue: 0.0, cost: 0.0, name, category: cat_name, swatch }
            });
            entry.qty += it.quantity;
            entry.revenue += o.total.to_f64().unwrap_or(0.0) * share;
            if let Some(c) = product_by_id.get(&it.product_id).and_then(|p| p.cost_price) {
                entry.cost += c.to_f64().unwrap_or(0.0) * it.quantity;
            }
        }
    }

    let total_units: f64 = by_pid.values().map(|a| a.qty).sum();
    let total_revenue: f64 = by_pid.values().map(|a| a.revenue).sum();
    let total_cost: f64 = by_pid.values().map(|a| a.cost).sum();
    let margin_pct = if total_revenue > 0.0 {
        (total_revenue - total_cost) / total_revenue * 100.0
    } else {
        0.0
    };

    // Top por qty (SKU mais vendido) e por receita.
    let top_qty = by_pid.values().max_by(|a, b| a.qty.partial_cmp(&b.qty).unwrap_or(std::cmp::Ordering::Equal));
    let top_rev = by_pid.values().max_by(|a, b| a.revenue.partial_cmp(&b.revenue).unwrap_or(std::cmp::Ordering::Equal));

    snap.kpis = vec![
        kpi(
            "ITENS VENDIDOS",
            &format!("{}", total_units.round() as i64),
            "Unidades no período",
            Color::from_rgb_u8(0xE6, 0x51, 0x00),
            "neutral",
            "inventory",
            true,
        ),
        kpi(
            "MAIS VENDIDO",
            top_qty.map(|a| a.name.as_str()).unwrap_or(""),
            &top_qty.map(|a| format!("{} Unidades", a.qty.round() as i64)).unwrap_or_else(|| "".into()),
            Color::from_rgb_u8(0x1E, 0x88, 0xE5),
            "neutral",
            "desempenho",
            true,
        ),
        kpi(
            "MAIOR RECEITA",
            &top_rev.map(|a| money_br(a.revenue)).unwrap_or_else(|| "".into()),
            top_rev.map(|a| a.name.as_str()).unwrap_or(""),
            Color::from_rgb_u8(0x2E, 0x7D, 0x32),
            "neutral",
            "atividade",
            false,
        ),
        kpi(
            "MARGEM MÉDIA",
            &format!("{:.0}%", margin_pct),
            "Ponderada por venda",
            Color::from_rgb_u8(0x8E, 0x24, 0xAA),
            "neutral",
            "cotacao",
            true,
        ),
    ];

    // Top produtos por receita.
    let mut all: Vec<(Uuid, Agg)> = by_pid.into_iter().collect();
    all.sort_by(|a, b| b.1.revenue.partial_cmp(&a.1.revenue).unwrap_or(std::cmp::Ordering::Equal));
    let max_rev = all.first().map(|(_, a)| a.revenue).unwrap_or(0.0).max(0.001);
    snap.top_products = all
        .iter()
        .take(9)
        .enumerate()
        .map(|(i, (pid, a))| {
            let image_b64 = product_by_id
                .get(pid)
                .and_then(|p| p.image_data.clone())
                .filter(|s| !s.is_empty());
            TopProductRaw {
                rank: (i + 1) as i32,
                name: a.name.clone(),
                category: a.category.clone(),
                qty_display: format!("{}", a.qty.round() as i64),
                revenue_display: money_br(a.revenue),
                progress: (a.revenue / max_rev) as f32,
                swatch_color: a.swatch,
                image_b64,
            }
        })
        .collect();
}

pub(crate) fn fill_customers(
    snap: &mut Snapshot,
    valid: &[&Order],
    all_orders: &[Order],
    customer_by_id: &HashMap<Uuid, &Customer>,
    start: NaiveDate,
    end: NaiveDate,
) {
    // Períodos para detectar "novos": cliente cujo PRIMEIRO pedido foi dentro [start..end].
    let mut first_order: HashMap<Uuid, NaiveDate> = HashMap::new();
    for o in all_orders {
        if o.base.deleted_at.is_some() || o.status == OrderStatus::Cancelled { continue; }
        if o.customer_id.is_nil() { continue; }
        let d = o.base.created_at.date();
        first_order
            .entry(o.customer_id)
            .and_modify(|cur| { if d < *cur { *cur = d; } })
            .or_insert(d);
    }

    // Clientes ativos no período (com pedido no período).
    let mut active: HashMap<Uuid, ()> = HashMap::new();
    for o in valid {
        if !o.customer_id.is_nil() {
            active.insert(o.customer_id, ());
        }
    }
    let active_count = active.len();

    // Novos / recorrentes:
    let mut new_count = 0;
    let mut returning_count = 0;
    for cid in active.keys() {
        let first = first_order.get(cid).copied();
        if let Some(d) = first {
            if d >= start && d <= end {
                new_count += 1;
            } else {
                returning_count += 1;
            }
        }
    }
    let return_rate = if active_count > 0 {
        (returning_count as f64 / active_count as f64) * 100.0
    } else {
        0.0
    };

    // LTV total: soma de receita por cliente (todos os pedidos válidos).
    let mut ltv: HashMap<Uuid, (f64, i64)> = HashMap::new();
    for o in all_orders {
        if o.base.deleted_at.is_some() || o.status == OrderStatus::Cancelled { continue; }
        if o.customer_id.is_nil() { continue; }
        let entry = ltv.entry(o.customer_id).or_insert((0.0, 0));
        entry.0 += o.total.to_f64().unwrap_or(0.0);
        entry.1 += 1;
    }
    let total_customers = ltv.len() as f64;
    let total_ltv: f64 = ltv.values().map(|(r, _)| *r).sum();
    let avg_ltv = if total_customers > 0.0 { total_ltv / total_customers } else { 0.0 };

    snap.kpis = vec![
        kpi(
            "CLIENTES ATIVOS",
            &active_count.to_string(),
            "Compram",
            Color::from_rgb_u8(0xE6, 0x51, 0x00),
            "neutral",
            "customers",
            true,
        ),
        kpi(
            "NOVOS NO PERÍODO",
            &new_count.to_string(),
            "Primeira Compra",
            Color::from_rgb_u8(0x1E, 0x88, 0xE5),
            "neutral",
            "user",
            true,
        ),
        kpi(
            "TAXA DE RETORNO",
            &format!("{:.0}%", return_rate),
            "Voltaram a Comprar",
            Color::from_rgb_u8(0x2E, 0x7D, 0x32),
            if return_rate > 30.0 { "pos" } else { "neutral" },
            "atualizar",
            false,
        ),
        kpi(
            "LTV MÉDIO",
            &money_br(avg_ltv),
            "Receita por Cliente",
            Color::from_rgb_u8(0xC2, 0x18, 0x5B),
            "neutral",
            "pay-carteira",
            true,
        ),
    ];

    // Top clientes por LTV.
    let mut top: Vec<(Uuid, f64, i64)> = ltv
        .into_iter()
        .map(|(k, (r, c))| (k, r, c))
        .collect();
    top.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let max_top = top.first().map(|(_, r, _)| *r).unwrap_or(0.0).max(0.001);
    snap.top_customers = top
        .iter()
        .take(8)
        .map(|(cid, rev, count)| {
            let cust = customer_by_id.get(cid);
            let name = cust.map(|c| c.name.clone()).unwrap_or_else(|| "Sem nome".into());
            let initial = name.chars().next().map(|c| c.to_ascii_uppercase().to_string()).unwrap_or_else(|| "?".into());
            let photo_b64 = cust
                .and_then(|c| c.profile_picture.clone())
                .filter(|s| !s.is_empty());
            TopCustomerRaw {
                initial,
                name: name.clone(),
                orders_display: format!("{} Pedidos", count),
                revenue_display: money_br(*rev),
                progress: (*rev / max_top) as f32,
                is_vip: *rev >= avg_ltv * 2.0,
                initial_color: color_for(&name),
                photo_b64,
            }
        })
        .collect();

    // Novos vs recorrentes
    let total_seg = (new_count + returning_count).max(1) as f64;
    let new_pct = (new_count as f64 / total_seg) * 100.0;
    let ret_pct = (returning_count as f64 / total_seg) * 100.0;
    snap.new_vs_ret = ReportNewVsReturning {
        new_count,
        new_pct: SharedString::from(format!("{:.0}%", new_pct)),
        new_progress: (new_count as f64 / total_seg) as f32,
        returning_count,
        returning_pct: SharedString::from(format!("{:.0}%", ret_pct)),
        returning_progress: (returning_count as f64 / total_seg) as f32,
    };
}

