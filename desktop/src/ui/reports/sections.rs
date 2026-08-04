//! Builders de cada sub-relatório — SÓ APRESENTAÇÃO (§1/§3).
//!
//! Todo o cálculo (DRE, margens, ticket médio, rankings) vive em
//! `letaf_core::report`; aqui a UI apenas formata os números em rótulos,
//! cores e barras.

use std::collections::HashMap;

use chrono::NaiveDate;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use slint::{Color, SharedString};
use uuid::Uuid;

use letaf_core::customer::model::Customer;
use letaf_core::order::model::Order;
use letaf_core::product::model::Product;
use letaf_core::product::stock_movement::StockMovement;
use letaf_core::report::{
    CustomerMetrics, FinancialMetrics, OrdersMetrics, ProductMetrics,
};

use crate::format::money_br;
use crate::{ReportHBar, ReportHourlyBar, ReportNewVsReturning, ReportStockRow};

use super::super::helpers::half_donut_arc;
use super::helpers::{
    build_daily, color_for, count_progress, dre, kpi, money_plain, prep_sub, prep_value,
    progress_of, ChartWindow,
};
use super::snapshot::{Snapshot, TopCustomerRaw, TopProductRaw};

/// Percentual arredondado como a UI mostra (`Decimal` → `{:.0}%`).
fn pct(v: Decimal) -> String {
    format!("{:.0}%", v.to_f64().unwrap_or(0.0))
}

// ── Builders por sub-relatório ───────────────────────────────────

pub(crate) fn fill_financial(
    snap: &mut Snapshot,
    m: &FinancialMetrics,
    fiado_total: Decimal,
    valid: &[&Order],
    prev_valid: &[&Order],
    win: ChartWindow,
    period_days: i64,
) {
    snap.kpis = vec![
        kpi(
            "RECEITA BRUTA",
            &money_br(m.revenue),
            &format!("{} Pedidos · {} Dias", m.orders_count, period_days),
            Color::from_rgb_u8(0x2E, 0x7D, 0x32),
            "neutral",
            "atividade",
            false,
        ),
        kpi(
            "FIADOS",
            &money_br(fiado_total),
            "Pedidos fiados em aberto",
            // Laranja da marca — dívida pendente, não receita.
            Color::from_rgb_u8(0xE8, 0x73, 0x1C),
            "neutral",
            "pay-carteira",
            false,
        ),
        kpi(
            "CUSTOS",
            &money_br(m.cost),
            "Custo Produtos",
            Color::from_rgb_u8(0xE5, 0x39, 0x35),
            "neutral",
            "saida-estoque",
            false,
        ),
        kpi(
            "LUCRO LÍQUIDO",
            &money_br(m.net),
            &format!("Margem {}", pct(m.margin_pct)),
            Color::from_rgb_u8(0x43, 0xA0, 0x47),
            "neutral",
            "pay-carteira",
            false,
        ),
        kpi(
            "TICKET MÉDIO",
            &money_br(m.avg_ticket),
            &format!("{} Pedidos no período", m.orders_count),
            Color::from_rgb_u8(0x2E, 0x7D, 0x32),
            "neutral",
            "coupons",
            true,
        ),
    ];

    // Receita diária (gráfico) — tooltip sem prefixo "R$ " (estava
    // cortando dentro da pílula do candle).
    snap.daily_bars = build_daily(
        win,
        valid,
        prev_valid,
        |o| o.total,
        money_plain,
        Color::from_rgb_u8(0x66, 0xBB, 0x6A),
    );

    // DRE simplificada — somente linhas com dados disponíveis no
    // domínio. Despesas/Taxas/Impostos serão adicionadas quando
    // entrarem como entidades persistidas.
    snap.dre_lines = vec![
        dre("Receita Bruta", &format!("+{}", money_br(m.revenue)), "pos"),
        dre("Custo de Produtos", &format!("−{}", money_br(m.cost)), "neg"),
        dre("LUCRO LÍQUIDO", &money_br(m.net), "total"),
    ];

    fill_payment_methods(snap, m);
}

/// Gauge de recebimentos por forma de pagamento.
///
/// Ordem solicitada: Dinheiro, PIX, Cartão Crédito, Cartão Débito.
/// Cores iguais ao Dashboard: Dinheiro verde, PIX azul, Crédito
/// vermelho, Débito amarelo. O 100% do gauge é a soma das formas
/// CONHECIDAS (pedidos sem método ficam fora).
fn fill_payment_methods(snap: &mut Snapshot, m: &FinancialMetrics) {
    let palette = [
        ("Dinheiro", "pay-dinheiro", Color::from_rgb_u8(0x2E, 0x7D, 0x32), m.methods.cash),
        ("PIX", "pay-pix", Color::from_rgb_u8(0x1E, 0x88, 0xE5), m.methods.pix),
        ("Cartão Crédito", "pay-cartao-credito", Color::from_rgb_u8(0xE5, 0x39, 0x35), m.methods.credit),
        ("Cartão Débito", "pay-cartao-debito", Color::from_rgb_u8(0xF9, 0xA8, 0x25), m.methods.debit),
    ];
    let total = m.methods.total();
    snap.method_total = money_br(total);
    // Acumulador de fração para encadear os arcos da meia-lua (gauge).
    let mut acc = 0.0_f64;
    let mut bars = Vec::new();
    for (label, icon, color, value) in palette {
        // Sem NENHUM pedido no período as 4 formas aparecem zeradas
        // (gauge vazio); havendo pedidos, só entra quem recebeu algo.
        if value <= Decimal::ZERO && m.orders_count > 0 {
            continue;
        }
        let frac = if total > Decimal::ZERO {
            (value / total).to_f64().unwrap_or(0.0)
        } else {
            0.0
        };
        let arc = half_donut_arc(acc, acc + frac);
        acc += frac;
        bars.push(ReportHBar {
            label: SharedString::from(label),
            value_display: SharedString::from(money_br(value)),
            progress: frac as f32,
            bar_color: color,
            icon_key: SharedString::from(icon),
            arc_commands: SharedString::from(arc),
        });
    }
    snap.method_bars = bars;
}

pub(crate) fn fill_orders(
    snap: &mut Snapshot,
    m: &OrdersMetrics,
    in_window: &[&Order],
    prev_in_window: &[&Order],
    win: ChartWindow,
) {
    snap.kpis = vec![
        kpi(
            "TOTAL DE PEDIDOS",
            &m.total.to_string(),
            &format!("{} Válidos · {} Cancelados", m.valid, m.cancelled),
            Color::from_rgb_u8(0xE6, 0x51, 0x00),
            "neutral",
            "orders",
            true,
        ),
        kpi(
            "TICKET MÉDIO",
            &money_br(m.avg_ticket),
            "Receita por pedido",
            Color::from_rgb_u8(0x2E, 0x7D, 0x32),
            "neutral",
            "coupons",
            false,
        ),
        kpi(
            "TAXA CANCELAMENTO",
            &format!("{:.1}%", m.cancel_rate.to_f64().unwrap_or(0.0)),
            &format!("{} Cancelados", m.cancelled),
            Color::from_rgb_u8(0xE5, 0x39, 0x35),
            "neutral",
            "nao-conformidade",
            false,
        ),
        kpi(
            "TEMPO MÉDIO PREPARO",
            &prep_value(m.avg_prep_minutes),
            &prep_sub(m.completed_count),
            Color::from_rgb_u8(0x1E, 0x88, 0xE5),
            "neutral",
            "relogio",
            true,
        ),
    ];

    // Pedidos por dia (cada pedido pesa 1).
    snap.orders_bars = build_daily(
        win,
        in_window,
        prev_in_window,
        |_| Decimal::ONE,
        |v| format!("{}", v.round()),
        Color::from_rgb_u8(0xFB, 0x8C, 0x00),
    );

    snap.channel_bars = channel_bars(m);
    snap.hourly_bars = hourly_bars(m);
}

/// Barras "por canal" — ordem solicitada: Balcão, Entrega, Retirada.
/// Cores: Balcão verde, Entrega azul, Retirada laranja.
fn channel_bars(m: &OrdersMetrics) -> Vec<ReportHBar> {
    let c = m.channels;
    let max = c.pdv.max(c.delivery).max(c.pickup).max(1);
    [
        ("Balcão", "pdv", Color::from_rgb_u8(0x2E, 0x7D, 0x32), c.pdv),
        ("Entrega", "rastreamento", Color::from_rgb_u8(0x1E, 0x88, 0xE5), c.delivery),
        ("Retirada", "recebimento", Color::from_rgb_u8(0xE6, 0x51, 0x00), c.pickup),
    ]
    .into_iter()
    .map(|(label, icon, color, count)| ReportHBar {
        label: SharedString::from(label),
        value_display: SharedString::from(count.to_string()),
        progress: count_progress(count, max),
        bar_color: color,
        icon_key: SharedString::from(icon),
        arc_commands: SharedString::new(),
    })
    .collect()
}

/// Pedidos por horário (08h..22h) — atual × período anterior, na mesma
/// escala (o máximo considera as duas séries).
fn hourly_bars(m: &OrdersMetrics) -> Vec<ReportHourlyBar> {
    let max = m
        .by_hour
        .iter()
        .chain(m.prev_by_hour.iter())
        .copied()
        .max()
        .unwrap_or(0);
    (8..23_usize)
        .map(|h| {
            let label = if h % 2 == 0 { format!("{:02}h", h) } else { String::new() };
            let (count, prev) = (m.by_hour[h], m.prev_by_hour[h]);
            ReportHourlyBar {
                label: SharedString::from(label),
                progress: count_progress(count, max),
                value_display: SharedString::from(count_label(count)),
                previous_progress: count_progress(prev, max),
                previous_display: SharedString::from(count_label(prev)),
            }
        })
        .collect()
}

/// Contagem exibida na barra — zero não escreve nada.
fn count_label(count: u32) -> String {
    if count > 0 {
        count.to_string()
    } else {
        String::new()
    }
}

pub(crate) fn fill_products(
    snap: &mut Snapshot,
    m: &ProductMetrics,
    product_by_id: &HashMap<Uuid, &Product>,
) {
    snap.kpis = vec![
        kpi(
            "ITENS VENDIDOS",
            &format!("{}", m.total_units.round() as i64),
            "Unidades no período",
            Color::from_rgb_u8(0xE6, 0x51, 0x00),
            "neutral",
            "inventory",
            true,
        ),
        kpi(
            "MAIS VENDIDO",
            m.top_by_quantity.as_ref().map(|a| a.name.as_str()).unwrap_or(""),
            &m.top_by_quantity
                .as_ref()
                .map(|a| format!("{} Unidades", a.quantity.round() as i64))
                .unwrap_or_default(),
            Color::from_rgb_u8(0x1E, 0x88, 0xE5),
            "neutral",
            "desempenho",
            true,
        ),
        kpi(
            "MAIOR RECEITA",
            &m.top_by_revenue
                .as_ref()
                .map(|a| money_br(a.revenue))
                .unwrap_or_default(),
            m.top_by_revenue.as_ref().map(|a| a.name.as_str()).unwrap_or(""),
            Color::from_rgb_u8(0x2E, 0x7D, 0x32),
            "neutral",
            "atividade",
            false,
        ),
        kpi(
            "MARGEM MÉDIA",
            &pct(m.margin_pct),
            "Ponderada por venda",
            Color::from_rgb_u8(0x8E, 0x24, 0xAA),
            "neutral",
            "cotacao",
            true,
        ),
    ];

    // Top produtos por receita (a escala mínima evita divisão por zero).
    let max_rev = m
        .ranking
        .first()
        .map(|a| a.revenue)
        .unwrap_or(Decimal::ZERO)
        .max(Decimal::new(1, 3));
    snap.top_products = m
        .ranking
        .iter()
        .take(9)
        .enumerate()
        .map(|(i, a)| TopProductRaw {
            rank: (i + 1) as i32,
            name: a.name.clone(),
            category: a.category_name.clone(),
            qty_display: format!("{}", a.quantity.round() as i64),
            revenue_display: money_br(a.revenue),
            progress: progress_of(a.revenue, max_rev),
            swatch_color: color_for(&a.category_name),
            image_b64: product_by_id
                .get(&a.product_id)
                // Miniatura da lista (o `find_all` não traz `image_data`).
                .and_then(|p| p.thumb_data.clone().or_else(|| p.image_data.clone()))
                .filter(|s| !s.is_empty()),
        })
        .collect();
}

pub(crate) fn fill_customers(
    snap: &mut Snapshot,
    m: &CustomerMetrics,
    customer_by_id: &HashMap<Uuid, &Customer>,
) {
    let return_rate = m.return_rate.to_f64().unwrap_or(0.0);
    snap.kpis = vec![
        kpi(
            "CLIENTES ATIVOS",
            &m.active_count.to_string(),
            "Compram",
            Color::from_rgb_u8(0xE6, 0x51, 0x00),
            "neutral",
            "customers",
            true,
        ),
        kpi(
            "NOVOS NO PERÍODO",
            &m.new_count.to_string(),
            "Primeira Compra",
            Color::from_rgb_u8(0x1E, 0x88, 0xE5),
            "neutral",
            "user",
            true,
        ),
        kpi(
            "TAXA DE RETORNO",
            &pct(m.return_rate),
            "Voltaram a Comprar",
            Color::from_rgb_u8(0x2E, 0x7D, 0x32),
            if return_rate > 30.0 { "pos" } else { "neutral" },
            "atualizar",
            false,
        ),
        kpi(
            "LTV MÉDIO",
            &money_br(m.avg_ltv),
            "Receita por Cliente",
            Color::from_rgb_u8(0xC2, 0x18, 0x5B),
            "neutral",
            "pay-carteira",
            true,
        ),
    ];

    snap.top_customers = top_customers(m, customer_by_id);

    // Novos vs recorrentes
    let total_seg = (m.new_count + m.returning_count).max(1) as f64;
    snap.new_vs_ret = ReportNewVsReturning {
        new_count: m.new_count,
        new_pct: SharedString::from(format!("{:.0}%", m.new_count as f64 / total_seg * 100.0)),
        new_progress: (m.new_count as f64 / total_seg) as f32,
        returning_count: m.returning_count,
        returning_pct: SharedString::from(format!(
            "{:.0}%",
            m.returning_count as f64 / total_seg * 100.0
        )),
        returning_progress: (m.returning_count as f64 / total_seg) as f32,
    };
}

/// Top 8 clientes por LTV — nome, inicial e foto vêm do cadastro.
fn top_customers(
    m: &CustomerMetrics,
    customer_by_id: &HashMap<Uuid, &Customer>,
) -> Vec<TopCustomerRaw> {
    let max = m
        .ranking
        .first()
        .map(|a| a.revenue)
        .unwrap_or(Decimal::ZERO)
        .max(Decimal::new(1, 3));
    m.ranking
        .iter()
        .take(8)
        .map(|a| {
            let cust = customer_by_id.get(&a.customer_id);
            let name = cust.map(|c| c.name.clone()).unwrap_or_else(|| "Sem nome".into());
            let initial = name
                .chars()
                .next()
                .map(|c| c.to_ascii_uppercase().to_string())
                .unwrap_or_else(|| "?".into());
            TopCustomerRaw {
                initial,
                name: name.clone(),
                orders_display: format!("{} Pedidos", a.orders),
                revenue_display: money_br(a.revenue),
                progress: progress_of(a.revenue, max),
                is_vip: a.is_vip,
                initial_color: color_for(&name),
                photo_b64: cust
                    .and_then(|c| c.profile_picture.clone())
                    .filter(|s| !s.is_empty()),
            }
        })
        .collect()
}

// ── ESTOQUE (extrato de movimentações) ───────────────────────────
// Máximo de linhas renderizadas na lista. As KPIs contam TODAS as
// movimentações do período; a lista mostra as mais recentes. Evita
// materializar milhares de elementos Slint num período longo.
const MAX_STOCK_ROWS: usize = 300;

/// Rótulo e cor do selo de cada tipo de movimento (`reason`).
fn stock_kind(reason: &str) -> (&'static str, Color) {
    match reason {
        "sale" => ("Venda", Color::from_rgb_u8(0x1E, 0x88, 0xE5)),
        "cancel" => ("Cancelamento", Color::from_rgb_u8(0xF5, 0x7F, 0x17)),
        "adjust" => ("Ajuste", Color::from_rgb_u8(0xE8, 0x73, 0x1C)),
        "consumo" => ("Consumo", Color::from_rgb_u8(0x8E, 0x24, 0xAA)),
        "edit" => ("Edição de venda", Color::from_rgb_u8(0x78, 0x90, 0x9C)),
        _ => ("Movimento", Color::from_rgb_u8(0x9E, 0x9E, 0x9E)),
    }
}

/// Quantidade com sinal e vírgula decimal: 5.0 → "+5", -1.5 → "−1,5".
fn fmt_signed_qty(delta: f64) -> String {
    let sign = if delta < 0.0 { "−" } else { "+" };
    let abs = format!("{}", delta.abs()).replace('.', ",");
    format!("{sign}{abs}")
}

/// Preenche o extrato de estoque: recorta o ledger pelo período (data
/// local), ordena do mais recente ao mais antigo, resolve o nome do
/// produto pelo índice e formata cada linha. As KPIs somam entradas e
/// saídas de TODO o período (§1/§3 — só apresentação).
pub(crate) fn fill_stock(
    snap: &mut Snapshot,
    movements: &[StockMovement],
    product_by_id: &HashMap<Uuid, &Product>,
    start: NaiveDate,
    end: NaiveDate,
) {
    // Recorte por data LOCAL da loja (o created_at é UTC).
    let mut janela: Vec<&StockMovement> = movements
        .iter()
        .filter(|m| {
            let d = letaf_core::tz::to_local(m.base.created_at).date();
            d >= start && d <= end
        })
        .collect();
    // Mais recente primeiro.
    janela.sort_by_key(|m| std::cmp::Reverse(m.base.created_at));

    let entradas: f64 = janela.iter().filter(|m| m.delta > 0.0).map(|m| m.delta).sum();
    let saidas: f64 = janela.iter().filter(|m| m.delta < 0.0).map(|m| -m.delta).sum();
    let n_entradas = janela.iter().filter(|m| m.delta > 0.0).count();
    let n_saidas = janela.iter().filter(|m| m.delta < 0.0).count();

    snap.kpis = vec![
        kpi(
            "ENTRADAS",
            &fmt_signed_qty(entradas),
            &format!("{n_entradas} movimentos"),
            Color::from_rgb_u8(0x2E, 0x7D, 0x32),
            "neutral",
            "atividade",
            false,
        ),
        kpi(
            "SAÍDAS",
            &fmt_signed_qty(-saidas),
            &format!("{n_saidas} movimentos"),
            Color::from_rgb_u8(0xC6, 0x28, 0x28),
            "neutral",
            "saida-estoque",
            false,
        ),
        kpi(
            "MOVIMENTOS",
            &format!("{}", janela.len()),
            "No período",
            Color::from_rgb_u8(0x1E, 0x88, 0xE5),
            "neutral",
            "inventory",
            false,
        ),
    ];

    let green = Color::from_rgb_u8(0x2E, 0x7D, 0x32);
    let red = Color::from_rgb_u8(0xC6, 0x28, 0x28);
    snap.stock_rows = janela
        .iter()
        .take(MAX_STOCK_ROWS)
        .map(|m| {
            let (label, kind_color) = stock_kind(&m.reason);
            let name = product_by_id
                .get(&m.product_id)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| "Produto removido".into());
            let local = letaf_core::tz::to_local(m.base.created_at);
            ReportStockRow {
                date_display: SharedString::from(local.format("%d/%m · %H:%M").to_string()),
                product_name: SharedString::from(name),
                kind_label: SharedString::from(label),
                kind_color,
                qty_display: SharedString::from(fmt_signed_qty(m.delta)),
                qty_color: if m.delta < 0.0 { red } else { green },
            }
        })
        .collect();
}
