//! Analytics do dashboard: agregações puras sobre pedidos (§3 — a regra de
//! negócio vive aqui, não na UI). Determinístico dada a lista de pedidos e a
//! data de referência (`today`) — sem relógio, para ser testável.

use std::collections::HashMap;

use chrono::{Datelike, Duration, NaiveDate, Timelike};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

use super::model::{
    ComparePoint, DashboardMetrics, DashboardPeriod, PaymentBreakdown, TimeBucket, TopProduct,
};
use crate::order::model::{Order, OrderStatus};

/// Ponto de entrada: calcula todas as métricas do dashboard.
pub fn compute(orders: &[Order], today: NaiveDate, period: DashboardPeriod) -> DashboardMetrics {
    // Pedidos válidos: não removidos e não cancelados.
    let valid: Vec<&Order> = orders
        .iter()
        .filter(|o| o.base.deleted_at.is_none() && o.status != OrderStatus::Cancelled)
        .collect();

    let (rev_today, rev_today_delta, orders_today, orders_delta, ticket, ticket_delta) =
        kpis(&valid, today);

    let (win_start, prev_start, prev_end) = period_window(today, period);
    let in_win = |d: NaiveDate| d >= win_start && d <= today;

    let period_revenue = rev_between(&valid, win_start, today);
    let prev_rev = rev_between(&valid, prev_start, prev_end);
    let period_orders = count_between(&valid, win_start, today);
    let period_ticket = safe_ticket(period_revenue, period_orders);

    DashboardMetrics {
        revenue_today: rev_today,
        revenue_today_delta: rev_today_delta,
        orders_today,
        orders_today_delta: orders_delta,
        avg_ticket_today: ticket,
        avg_ticket_delta: ticket_delta,
        sales_week: build_sales_week(&valid, today),
        compare: build_compare(&valid, today, period),
        period_series: build_period_series(&valid, today, period),
        period_revenue,
        period_revenue_delta: pct_delta(d2f(period_revenue), d2f(prev_rev)),
        period_orders,
        period_ticket,
        period_best_day: best_day(&valid, in_win),
        top_products: top_products(&valid, in_win),
        payments: payment_breakdown(&valid, in_win),
    }
}

/// KPIs de "hoje": receita, pedidos e ticket médio, cada um com seu delta %.
fn kpis(
    valid: &[&Order],
    today: NaiveDate,
) -> (Decimal, Option<f64>, u32, Option<f64>, Decimal, Option<f64>) {
    let same_day_last_week = today - Duration::days(7);

    let rev_today = rev_on(valid, today);
    let rev_baseline = rev_on(valid, same_day_last_week);
    let rev_delta = pct_delta(d2f(rev_today), d2f(rev_baseline));

    let orders_today = count_on(valid, today);
    let orders_baseline = count_on(valid, same_day_last_week);
    let orders_delta = pct_delta(orders_today as f64, orders_baseline as f64);

    let ticket_today = safe_ticket(rev_today, orders_today);
    // Base do ticket: média dos últimos 7 dias (excluindo hoje).
    let week_start = today - Duration::days(7);
    let last7_rev = rev_between(valid, week_start, today - Duration::days(1));
    let last7_count = count_between(valid, week_start, today - Duration::days(1));
    let last7_avg = safe_ticket(last7_rev, last7_count);
    let ticket_delta = pct_delta(d2f(ticket_today), d2f(last7_avg));

    (rev_today, rev_delta, orders_today, orders_delta, ticket_today, ticket_delta)
}

/// Vendas da semana corrente: segunda a domingo (7 buckets por dia).
fn build_sales_week(valid: &[&Order], today: NaiveDate) -> Vec<TimeBucket> {
    let monday = monday_of(today);
    (0..7)
        .map(|i| {
            let d = monday + Duration::days(i);
            TimeBucket { date: d, hour: None, revenue: rev_on(valid, d) }
        })
        .collect()
}

/// Comparativo período atual vs anterior, com buckets conforme o filtro:
/// hoje→horas (24), semana→dias (7), mês→dias do mês.
fn build_compare(valid: &[&Order], today: NaiveDate, period: DashboardPeriod) -> Vec<ComparePoint> {
    match period {
        DashboardPeriod::Today => {
            let yest = today - Duration::days(1);
            (0..24u32)
                .map(|h| ComparePoint {
                    date: today,
                    hour: Some(h),
                    current: rev_hour(valid, today, h),
                    previous: rev_hour(valid, yest, h),
                })
                .collect()
        }
        DashboardPeriod::Month => {
            let first = today.with_day(1).unwrap_or(today);
            let prev_last = first - Duration::days(1);
            let prev_first = prev_last.with_day(1).unwrap_or(prev_last);
            let days = days_in_month(today.year(), today.month());
            let prev_days = days_in_month(prev_first.year(), prev_first.month());
            (1..=days)
                .map(|d| {
                    let cur = first.with_day(d).unwrap_or(first);
                    let prev = prev_first.with_day(d.min(prev_days)).unwrap_or(prev_first);
                    ComparePoint {
                        date: cur,
                        hour: None,
                        current: rev_on(valid, cur),
                        previous: rev_on(valid, prev),
                    }
                })
                .collect()
        }
        DashboardPeriod::Week => {
            let monday = monday_of(today);
            (0..7)
                .map(|i| {
                    let cur = monday + Duration::days(i);
                    let prev = cur - Duration::days(7);
                    ComparePoint {
                        date: cur,
                        hour: None,
                        current: rev_on(valid, cur),
                        previous: rev_on(valid, prev),
                    }
                })
                .collect()
        }
    }
}

/// Série de 7 buckets do hero: hoje→faixas de 2h (08h..20h), semana→dias
/// (seg..dom), mês→faixas de dias.
fn build_period_series(
    valid: &[&Order],
    today: NaiveDate,
    period: DashboardPeriod,
) -> Vec<TimeBucket> {
    match period {
        DashboardPeriod::Today => (0..7)
            .map(|i| {
                let h0 = 8 + i * 2;
                let revenue = valid
                    .iter()
                    .filter(|o| {
                        let dt = o.base.created_at;
                        dt.date() == today && (dt.hour() as i64) >= h0 && (dt.hour() as i64) < h0 + 2
                    })
                    .map(|o| o.total)
                    .sum();
                TimeBucket { date: today, hour: Some(h0 as u32), revenue }
            })
            .collect(),
        DashboardPeriod::Month => {
            let first = today.with_day(1).unwrap_or(today);
            let span = (((today.day() as i64) + 6) / 7).max(1);
            (0..7)
                .map(|i| {
                    let d0 = first + Duration::days(i * span);
                    let d1 = first + Duration::days((i + 1) * span);
                    let revenue = valid
                        .iter()
                        .filter(|o| {
                            let d = o.base.created_at.date();
                            d >= d0 && d < d1 && d <= today
                        })
                        .map(|o| o.total)
                        .sum();
                    TimeBucket { date: d0, hour: None, revenue }
                })
                .collect()
        }
        DashboardPeriod::Week => {
            let monday = monday_of(today);
            (0..7)
                .map(|i| {
                    let d = monday + Duration::days(i);
                    TimeBucket { date: d, hour: None, revenue: rev_on(valid, d) }
                })
                .collect()
        }
    }
}

/// Top 5 produtos por receita na janela (`in_win`), com quantidade somada.
fn top_products(valid: &[&Order], in_win: impl Fn(NaiveDate) -> bool) -> Vec<TopProduct> {
    let mut prod: HashMap<String, (Decimal, f64)> = HashMap::new();
    for o in valid.iter().filter(|o| in_win(o.base.created_at.date())) {
        for it in &o.items {
            let e = prod.entry(it.product_name.clone()).or_insert((Decimal::ZERO, 0.0));
            e.0 += it.subtotal;
            e.1 += it.quantity;
        }
    }
    let mut vec: Vec<TopProduct> = prod
        .into_iter()
        .map(|(name, (revenue, quantity))| TopProduct { name, revenue, quantity })
        .collect();
    vec.sort_by_key(|p| std::cmp::Reverse(p.revenue));
    vec.truncate(5);
    vec
}

/// Receita por forma de pagamento na janela (carteira/sem método ficam fora).
fn payment_breakdown(valid: &[&Order], in_win: impl Fn(NaiveDate) -> bool) -> PaymentBreakdown {
    let (mut pix, mut credit, mut debit, mut cash) =
        (Decimal::ZERO, Decimal::ZERO, Decimal::ZERO, Decimal::ZERO);
    for o in valid.iter().filter(|o| in_win(o.base.created_at.date())) {
        match o.payment_method.as_deref() {
            Some("pix") => pix += o.total,
            Some("credit") => credit += o.total,
            Some("debit") => debit += o.total,
            Some("cash") => cash += o.total,
            _ => {}
        }
    }
    PaymentBreakdown { pix, credit, debit, cash }
}

/// Dia de maior receita na janela; `None` se não houve receita.
fn best_day(valid: &[&Order], in_win: impl Fn(NaiveDate) -> bool) -> Option<NaiveDate> {
    let mut by_day: HashMap<NaiveDate, Decimal> = HashMap::new();
    for o in valid.iter().filter(|o| in_win(o.base.created_at.date())) {
        *by_day.entry(o.base.created_at.date()).or_insert(Decimal::ZERO) += o.total;
    }
    by_day
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1))
        .filter(|(_, v)| *v > Decimal::ZERO)
        .map(|(d, _)| d)
}

// ── Janela do período ────────────────────────────────────────────────────

/// (início da janela atual, início e fim da janela anterior equivalente).
fn period_window(today: NaiveDate, period: DashboardPeriod) -> (NaiveDate, NaiveDate, NaiveDate) {
    match period {
        DashboardPeriod::Today => (today, today - Duration::days(1), today - Duration::days(1)),
        DashboardPeriod::Month => {
            let first = today.with_day(1).unwrap_or(today);
            let prev_last = first - Duration::days(1);
            (first, prev_last.with_day(1).unwrap_or(prev_last), prev_last)
        }
        DashboardPeriod::Week => {
            let monday = monday_of(today);
            (monday, monday - Duration::days(7), monday - Duration::days(1))
        }
    }
}

// ── Primitivos de soma/contagem ───────────────────────────────────────────

fn rev_on(valid: &[&Order], d: NaiveDate) -> Decimal {
    valid.iter().filter(|o| o.base.created_at.date() == d).map(|o| o.total).sum()
}

fn rev_between(valid: &[&Order], a: NaiveDate, b: NaiveDate) -> Decimal {
    valid
        .iter()
        .filter(|o| {
            let d = o.base.created_at.date();
            d >= a && d <= b
        })
        .map(|o| o.total)
        .sum()
}

fn rev_hour(valid: &[&Order], d: NaiveDate, h: u32) -> Decimal {
    valid
        .iter()
        .filter(|o| o.base.created_at.date() == d && o.base.created_at.hour() == h)
        .map(|o| o.total)
        .sum()
}

fn count_on(valid: &[&Order], d: NaiveDate) -> u32 {
    valid.iter().filter(|o| o.base.created_at.date() == d).count() as u32
}

fn count_between(valid: &[&Order], a: NaiveDate, b: NaiveDate) -> u32 {
    valid
        .iter()
        .filter(|o| {
            let d = o.base.created_at.date();
            d >= a && d <= b
        })
        .count() as u32
}

// ── Utilitários ────────────────────────────────────────────────────────────

/// Ticket médio protegido contra divisão por zero.
fn safe_ticket(revenue: Decimal, count: u32) -> Decimal {
    if count > 0 {
        revenue / Decimal::from(count)
    } else {
        Decimal::ZERO
    }
}

/// Variação percentual `current` vs `baseline`.
/// - baseline ~0 e current ~0 → `Some(0.0)` (estável);
/// - baseline ~0 e current ≠0 → `None` (sem base de comparação);
/// - caso geral → `Some((current-baseline)/baseline*100)`.
pub fn pct_delta(current: f64, baseline: f64) -> Option<f64> {
    if baseline.abs() < 0.005 {
        if current.abs() < 0.005 {
            Some(0.0)
        } else {
            None
        }
    } else {
        Some((current - baseline) / baseline * 100.0)
    }
}

/// Segunda-feira da semana que contém `d`.
fn monday_of(d: NaiveDate) -> NaiveDate {
    d - Duration::days(d.weekday().num_days_from_monday() as i64)
}

/// Número de dias do mês (calendário gregoriano).
pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

fn d2f(d: Decimal) -> f64 {
    d.to_f64().unwrap_or(0.0)
}
