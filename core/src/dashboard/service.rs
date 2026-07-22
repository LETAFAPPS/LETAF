//! Analytics do dashboard: agregações puras sobre pedidos (§3 — a regra de
//! negócio vive aqui, não na UI). Determinístico dada a lista de pedidos, a
//! data de referência (`today`) e o fuso da loja — sem relógio, para ser
//! testável.
//!
//! FUSO (§11): `created_at` é gravado em **UTC** (`BaseFields::new`), mas o
//! lojista raciocina no horário LOCAL. Todo agrupamento por dia/hora converte
//! com `utc_offset_minutes` da empresa — o mesmo fuso que o servidor usa em
//! `availability::local_now`. Sem isso, em BRT (-3) uma venda às 21h entrava
//! no dia seguinte, justamente no pico do jantar.
//!
//! DESEMPENHO (§13): os índices por dia/hora são montados em UMA passada e
//! todo o resto são consultas O(1) no mapa. Antes eram ~90 varreduras sobre a
//! lista inteira de pedidos (31 buckets × 2 no comparativo mensal).

use std::collections::HashMap;

use chrono::{Datelike, Duration, NaiveDate, Timelike};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

use super::model::{
    ComparePoint, DashboardMetrics, DashboardPeriod, PaymentBreakdown, TimeBucket, TopProduct,
};
use crate::order::model::{Order, OrderStatus};

/// Pedido válido já posicionado no fuso da loja.
struct LocalOrder<'a> {
    date: NaiveDate,
    order: &'a Order,
}

/// Índices montados numa passada: receita/contagem por dia e receita por hora.
struct Index<'a> {
    orders: Vec<LocalOrder<'a>>,
    by_day: HashMap<NaiveDate, (Decimal, u32)>,
    by_hour: HashMap<(NaiveDate, u32), Decimal>,
}

impl<'a> Index<'a> {
    fn build(orders: &'a [Order], utc_offset_minutes: i32) -> Self {
        let offset = Duration::minutes(utc_offset_minutes as i64);
        let mut by_day: HashMap<NaiveDate, (Decimal, u32)> = HashMap::new();
        let mut by_hour: HashMap<(NaiveDate, u32), Decimal> = HashMap::new();
        let mut list = Vec::new();

        for o in orders {
            if o.base.deleted_at.is_some() || o.status == OrderStatus::Cancelled {
                continue;
            }
            let local = o.base.created_at + offset;
            let (date, hour) = (local.date(), local.hour());
            let e = by_day.entry(date).or_insert((Decimal::ZERO, 0));
            e.0 += o.total;
            e.1 += 1;
            *by_hour.entry((date, hour)).or_insert(Decimal::ZERO) += o.total;
            list.push(LocalOrder { date, order: o });
        }
        Self { orders: list, by_day, by_hour }
    }

    fn rev_on(&self, d: NaiveDate) -> Decimal {
        self.by_day.get(&d).map(|e| e.0).unwrap_or(Decimal::ZERO)
    }
    fn count_on(&self, d: NaiveDate) -> u32 {
        self.by_day.get(&d).map(|e| e.1).unwrap_or(0)
    }
    fn rev_hour(&self, d: NaiveDate, h: u32) -> Decimal {
        self.by_hour.get(&(d, h)).copied().unwrap_or(Decimal::ZERO)
    }
    /// Soma o intervalo INCLUSIVO percorrendo os dias (≤31 no pior caso),
    /// em vez de varrer a lista de pedidos.
    fn rev_between(&self, a: NaiveDate, b: NaiveDate) -> Decimal {
        let mut total = Decimal::ZERO;
        let mut d = a;
        while d <= b {
            total += self.rev_on(d);
            d += Duration::days(1);
        }
        total
    }
    fn count_between(&self, a: NaiveDate, b: NaiveDate) -> u32 {
        let mut total = 0u32;
        let mut d = a;
        while d <= b {
            total = total.saturating_add(self.count_on(d));
            d += Duration::days(1);
        }
        total
    }
}

/// Ponto de entrada: calcula todas as métricas do dashboard.
///
/// `today` deve ser a data JÁ no fuso da loja; `utc_offset_minutes` é o mesmo
/// campo da empresa usado pelo servidor.
pub fn compute(
    orders: &[Order],
    today: NaiveDate,
    period: DashboardPeriod,
    utc_offset_minutes: i32,
) -> DashboardMetrics {
    let ix = Index::build(orders, utc_offset_minutes);

    let (rev_today, rev_today_delta, orders_today, orders_delta, ticket, ticket_delta) =
        kpis(&ix, today);

    let (win_start, prev_start, prev_end) = period_window(today, period);
    let in_win = |d: NaiveDate| d >= win_start && d <= today;

    let period_revenue = ix.rev_between(win_start, today);
    let prev_rev = ix.rev_between(prev_start, prev_end);
    let period_orders = ix.count_between(win_start, today);
    let period_ticket = safe_ticket(period_revenue, period_orders);

    DashboardMetrics {
        revenue_today: rev_today,
        revenue_today_delta: rev_today_delta,
        orders_today,
        orders_today_delta: orders_delta,
        avg_ticket_today: ticket,
        avg_ticket_delta: ticket_delta,
        sales_week: build_sales_week(&ix, today),
        compare: build_compare(&ix, today, period),
        period_series: build_period_series(&ix, today, period),
        period_revenue,
        period_revenue_delta: pct_delta(d2f(period_revenue), d2f(prev_rev)),
        period_orders,
        period_ticket,
        period_best_day: best_day(&ix, win_start, today),
        top_products: top_products(&ix, in_win),
        payments: payment_breakdown(&ix, in_win),
    }
}

/// KPIs de "hoje": receita, pedidos e ticket médio, cada um com seu delta %.
fn kpis(
    ix: &Index,
    today: NaiveDate,
) -> (Decimal, Option<f64>, u32, Option<f64>, Decimal, Option<f64>) {
    let same_day_last_week = today - Duration::days(7);

    let rev_today = ix.rev_on(today);
    let rev_delta = pct_delta(d2f(rev_today), d2f(ix.rev_on(same_day_last_week)));

    let orders_today = ix.count_on(today);
    let orders_delta = pct_delta(orders_today as f64, ix.count_on(same_day_last_week) as f64);

    let ticket_today = safe_ticket(rev_today, orders_today);
    // Base do ticket: média dos últimos 7 dias (excluindo hoje).
    let week_start = today - Duration::days(7);
    let last7_avg = safe_ticket(
        ix.rev_between(week_start, today - Duration::days(1)),
        ix.count_between(week_start, today - Duration::days(1)),
    );
    let ticket_delta = pct_delta(d2f(ticket_today), d2f(last7_avg));

    (rev_today, rev_delta, orders_today, orders_delta, ticket_today, ticket_delta)
}

/// Vendas da semana corrente: segunda a domingo (7 buckets por dia).
fn build_sales_week(ix: &Index, today: NaiveDate) -> Vec<TimeBucket> {
    let monday = monday_of(today);
    (0..7)
        .map(|i| {
            let d = monday + Duration::days(i);
            TimeBucket { date: d, hour: None, revenue: ix.rev_on(d) }
        })
        .collect()
}

/// Comparativo período atual vs anterior, com buckets conforme o filtro:
/// hoje→horas (24), semana→dias (7), mês→dias do mês.
fn build_compare(ix: &Index, today: NaiveDate, period: DashboardPeriod) -> Vec<ComparePoint> {
    match period {
        DashboardPeriod::Today => {
            let yest = today - Duration::days(1);
            (0..24u32)
                .map(|h| ComparePoint {
                    date: today,
                    hour: Some(h),
                    current: ix.rev_hour(today, h),
                    previous: ix.rev_hour(yest, h),
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
                    // Dia inexistente no mês anterior (ex.: 29..31 vs fevereiro)
                    // → ZERO. Antes fazia `min(prev_days)`, o que somava o
                    // último dia do mês curto várias vezes e inflava a série.
                    let previous = if d <= prev_days {
                        prev_first.with_day(d).map(|p| ix.rev_on(p)).unwrap_or(Decimal::ZERO)
                    } else {
                        Decimal::ZERO
                    };
                    ComparePoint { date: cur, hour: None, current: ix.rev_on(cur), previous }
                })
                .collect()
        }
        DashboardPeriod::Week => {
            let monday = monday_of(today);
            (0..7)
                .map(|i| {
                    let cur = monday + Duration::days(i);
                    ComparePoint {
                        date: cur,
                        hour: None,
                        current: ix.rev_on(cur),
                        previous: ix.rev_on(cur - Duration::days(7)),
                    }
                })
                .collect()
        }
    }
}

/// Série de 7 buckets do hero: hoje→faixas de 2h (08h..20h), semana→dias
/// (seg..dom), mês→faixas de dias já decorridos.
fn build_period_series(ix: &Index, today: NaiveDate, period: DashboardPeriod) -> Vec<TimeBucket> {
    match period {
        DashboardPeriod::Today => (0..7)
            .map(|i| {
                let h0 = 8 + i * 2;
                let revenue = ix.rev_hour(today, h0) + ix.rev_hour(today, h0 + 1);
                TimeBucket { date: today, hour: Some(h0), revenue }
            })
            .collect(),
        DashboardPeriod::Month => {
            let first = today.with_day(1).unwrap_or(today);
            let elapsed = today.day(); // dias já decorridos no mês
            (0..7u32)
                .map(|i| {
                    // Os 7 buckets particionam EXATAMENTE 1..=elapsed. Antes o
                    // passo era `ceil(elapsed/7)`, o que estourava o mês: os
                    // últimos buckets caíam no mês seguinte, saíam vazios e
                    // repetiam o rótulo "1".
                    let (lo, hi) = month_bucket(elapsed, i);
                    let revenue = if lo > hi {
                        Decimal::ZERO
                    } else {
                        let a = first.with_day(lo).unwrap_or(first);
                        let b = first.with_day(hi).unwrap_or(first);
                        ix.rev_between(a, b)
                    };
                    let date = first.with_day(lo.max(1)).unwrap_or(first);
                    TimeBucket { date, hour: None, revenue }
                })
                .collect()
        }
        DashboardPeriod::Week => {
            let monday = monday_of(today);
            (0..7)
                .map(|i| {
                    let d = monday + Duration::days(i);
                    TimeBucket { date: d, hour: None, revenue: ix.rev_on(d) }
                })
                .collect()
        }
    }
}

/// Faixa de dias (inclusiva) do bucket `i` de 7, cobrindo 1..=`elapsed`.
///
/// Até 7 dias decorridos é um dia por bucket (os futuros ficam vazios, com
/// rótulos distintos); acima disso os dias são distribuídos igualmente.
fn month_bucket(elapsed: u32, i: u32) -> (u32, u32) {
    if elapsed <= 7 {
        let d = i + 1;
        return (d, if d <= elapsed { d } else { 0 }); // lo > hi ⇒ bucket vazio
    }
    let lo = 1 + (i * elapsed) / 7;
    let hi = ((i + 1) * elapsed) / 7;
    (lo, hi)
}

/// Top 5 produtos por receita na janela, com quantidade somada.
/// Empate → ordem alfabética (determinístico).
fn top_products(ix: &Index, in_win: impl Fn(NaiveDate) -> bool) -> Vec<TopProduct> {
    let mut prod: HashMap<&str, (Decimal, f64)> = HashMap::new();
    for lo in ix.orders.iter().filter(|l| in_win(l.date)) {
        for it in &lo.order.items {
            let e = prod.entry(it.product_name.as_str()).or_insert((Decimal::ZERO, 0.0));
            e.0 += it.subtotal;
            e.1 += it.quantity;
        }
    }
    let mut vec: Vec<TopProduct> = prod
        .into_iter()
        .map(|(name, (revenue, quantity))| TopProduct {
            name: name.to_string(),
            revenue,
            quantity,
        })
        .collect();
    vec.sort_by(|a, b| b.revenue.cmp(&a.revenue).then_with(|| a.name.cmp(&b.name)));
    vec.truncate(5);
    vec
}

/// Receita por forma de pagamento na janela (carteira/sem método ficam fora).
fn payment_breakdown(ix: &Index, in_win: impl Fn(NaiveDate) -> bool) -> PaymentBreakdown {
    let (mut pix, mut credit, mut debit, mut cash) =
        (Decimal::ZERO, Decimal::ZERO, Decimal::ZERO, Decimal::ZERO);
    for lo in ix.orders.iter().filter(|l| in_win(l.date)) {
        match lo.order.payment_method.as_deref() {
            Some("pix") => pix += lo.order.total,
            Some("credit") => credit += lo.order.total,
            Some("debit") => debit += lo.order.total,
            Some("cash") => cash += lo.order.total,
            _ => {}
        }
    }
    PaymentBreakdown { pix, credit, debit, cash }
}

/// Dia de maior receita na janela; `None` se não houve receita.
/// Empate → o dia mais ANTIGO (determinístico; antes dependia da ordem do
/// `HashMap` e o "Melhor dia" alternava entre refreshes).
fn best_day(ix: &Index, win_start: NaiveDate, today: NaiveDate) -> Option<NaiveDate> {
    let mut best: Option<(NaiveDate, Decimal)> = None;
    let mut d = win_start;
    while d <= today {
        let rev = ix.rev_on(d);
        if rev > Decimal::ZERO {
            match best {
                Some((_, br)) if br >= rev => {}
                _ => best = Some((d, rev)),
            }
        }
        d += Duration::days(1);
    }
    best.map(|(d, _)| d)
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
