//! Helpers de APRESENTAÇÃO dos relatórios: buckets do gráfico, rótulos
//! e cores. Nenhuma regra de negócio aqui (§1/§3) — os números vêm de
//! `letaf_core::report`.
//!
//! Dinheiro é `Decimal` de ponta a ponta; só a formatação final converte
//! para texto (e a geometria das barras, para `f32`).

use chrono::{Datelike, Duration, NaiveDate, Timelike};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use slint::{Color, SharedString};

use letaf_core::order::model::Order;

use crate::{ReportDailyBar, ReportDreLine, ReportKpi, ReportOption};

use super::state::Granularity;

// ── Helpers ──────────────────────────────────────────────────────

pub(crate) fn opt(key: &str, label: &str, selected: bool) -> ReportOption {
    ReportOption {
        key: SharedString::from(key),
        label: SharedString::from(label),
        selected,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn kpi(
    label: &str,
    value: &str,
    sub: &str,
    accent: Color,
    tone: &str,
    icon: &str,
    muted: bool,
) -> ReportKpi {
    ReportKpi {
        label: SharedString::from(label),
        value_display: SharedString::from(value),
        sub_text: SharedString::from(sub),
        accent_color: accent,
        sub_tone: SharedString::from(tone),
        icon_key: SharedString::from(icon),
        value_muted: muted,
    }
}

pub(crate) fn dre(label: &str, value: &str, tone: &str) -> ReportDreLine {
    ReportDreLine {
        label: SharedString::from(label),
        value_display: SharedString::from(value),
        tone: SharedString::from(tone),
    }
}

/// Janela do gráfico: período ATUAL + o ANTERIOR equivalente.
///
/// O período anterior é o mesmo recorte deslocado (dia anterior, semana
/// anterior, mês anterior, ano anterior) — cada barra do gráfico ganha
/// um par de comparação na MESMA posição relativa.
#[derive(Clone, Copy)]
pub(crate) struct ChartWindow {
    pub(crate) start: NaiveDate,
    pub(crate) end: NaiveDate,
    pub(crate) today: NaiveDate,
    /// Início do período anterior (mesma duração do atual).
    pub(crate) prev_start: NaiveDate,
    pub(crate) granularity: Granularity,
}

/// Barras do gráfico principal (atual + anterior na mesma escala).
///
/// `orders` são os pedidos do período atual e `prev_orders` os do
/// anterior — ambos já filtrados por quem chama.
pub(crate) fn build_daily<F, G>(
    win: ChartWindow,
    orders: &[&Order],
    prev_orders: &[&Order],
    per_order: F,
    fmt: G,
    color: Color,
) -> Vec<ReportDailyBar>
where
    F: Fn(&Order) -> Decimal,
    G: Fn(Decimal) -> String,
{
    match win.granularity {
        Granularity::Hourly => build_hourly_buckets(win, orders, prev_orders, per_order, fmt, color),
        Granularity::Daily => build_daily_buckets(win, orders, prev_orders, per_order, fmt, color),
        Granularity::Monthly => build_monthly_buckets(win, orders, prev_orders, per_order, fmt, color),
    }
}

/// Um ponto por hora do dia (0..23) — atual e dia anterior.
fn build_hourly_buckets<F, G>(
    win: ChartWindow,
    orders: &[&Order],
    prev_orders: &[&Order],
    per_order: F,
    fmt: G,
    color: Color,
) -> Vec<ReportDailyBar>
where
    F: Fn(&Order) -> Decimal,
    G: Fn(Decimal) -> String,
{
    let now_hour = letaf_core::tz::now().hour();
    let cur = hour_totals(orders, win.today, &per_order);
    let prev = hour_totals(prev_orders, win.prev_start, &per_order);
    let max = series_max(&cur, &prev);
    (0..24_usize)
        .map(|h| {
            let label = if h % 3 == 0 { format!("{:02}h", h) } else { String::new() };
            bar(label, cur[h], prev[h], max, &fmt, color, h as u32 == now_hour)
        })
        .collect()
}

/// Um ponto por dia entre `start..end` — comparado ao mesmo índice de
/// dia do período anterior (semana passada / mês passado).
fn build_daily_buckets<F, G>(
    win: ChartWindow,
    orders: &[&Order],
    prev_orders: &[&Order],
    per_order: F,
    fmt: G,
    color: Color,
) -> Vec<ReportDailyBar>
where
    F: Fn(&Order) -> Decimal,
    G: Fn(Decimal) -> String,
{
    let span = ((win.end - win.start).num_days() + 1).max(1);
    let cur = day_totals(orders, win.start, span, &per_order);
    let prev = day_totals(prev_orders, win.prev_start, span, &per_order);
    let max = series_max(&cur, &prev);
    // Span ≤ 14 dias (semanal): weekday curto. Acima (mensal): TODOS
    // os números de dia do mês — o usuário pediu explicitamente.
    let use_weekday = span <= 14;
    (0..span as usize)
        .map(|i| {
            let d = win.start + Duration::days(i as i64);
            let label = if use_weekday {
                weekday_short(d).to_string()
            } else {
                format!("{:02}", d.day())
            };
            bar(label, cur[i], prev[i], max, &fmt, color, d == win.today)
        })
        .collect()
}

/// Um ponto por mês do ano corrente — comparado ao mesmo mês do ano
/// anterior.
fn build_monthly_buckets<F, G>(
    win: ChartWindow,
    orders: &[&Order],
    prev_orders: &[&Order],
    per_order: F,
    fmt: G,
    color: Color,
) -> Vec<ReportDailyBar>
where
    F: Fn(&Order) -> Decimal,
    G: Fn(Decimal) -> String,
{
    let year = win.today.year();
    let cur = month_totals(orders, year, &per_order);
    let prev = month_totals(prev_orders, win.prev_start.year(), &per_order);
    let max = series_max(&cur, &prev);
    let today_month = win.today.month() as usize;
    (1..=12_usize)
        .map(|m| {
            bar(
                month_short(m as u32).to_string(),
                cur[m - 1],
                prev[m - 1],
                max,
                &fmt,
                color,
                m == today_month,
            )
        })
        .collect()
}

// ── Agregação por bucket ─────────────────────────────────────────

fn hour_totals<F>(orders: &[&Order], day: NaiveDate, per_order: &F) -> Vec<Decimal>
where
    F: Fn(&Order) -> Decimal,
{
    let mut totals = vec![Decimal::ZERO; 24];
    for o in orders {
        // `created_at` é UTC (§6): converte para o fuso da loja antes de
        // comparar o dia/hora, senão o expediente das 21h em diante cai
        // no dia seguinte e as barras ficam 3h deslocadas.
        let local = letaf_core::tz::to_local(o.base.created_at);
        if local.date() == day {
            if let Some(slot) = totals.get_mut(local.hour() as usize) {
                *slot += per_order(o);
            }
        }
    }
    totals
}

fn day_totals<F>(orders: &[&Order], start: NaiveDate, span: i64, per_order: &F) -> Vec<Decimal>
where
    F: Fn(&Order) -> Decimal,
{
    let mut totals = vec![Decimal::ZERO; span as usize];
    for o in orders {
        let idx = (letaf_core::tz::to_local(o.base.created_at).date() - start).num_days();
        if idx >= 0 {
            if let Some(v) = totals.get_mut(idx as usize) {
                *v += per_order(o);
            }
        }
    }
    totals
}

fn month_totals<F>(orders: &[&Order], year: i32, per_order: &F) -> Vec<Decimal>
where
    F: Fn(&Order) -> Decimal,
{
    let mut totals = vec![Decimal::ZERO; 12];
    for o in orders {
        let d = letaf_core::tz::to_local(o.base.created_at).date();
        if d.year() == year {
            let m = d.month() as usize;
            if let Some(slot) = totals.get_mut(m - 1) {
                *slot += per_order(o);
            }
        }
    }
    totals
}

/// Maior valor entre as duas séries — as barras atual e anterior
/// dividem a MESMA escala vertical (senão a comparação engana).
fn series_max(cur: &[Decimal], prev: &[Decimal]) -> Decimal {
    cur.iter()
        .chain(prev.iter())
        .copied()
        .fold(Decimal::ZERO, Decimal::max)
}

/// Monta a barra (atual + anterior) já normalizada pela escala comum.
fn bar<G>(
    label: String,
    cur: Decimal,
    prev: Decimal,
    max: Decimal,
    fmt: &G,
    color: Color,
    highlight: bool,
) -> ReportDailyBar
where
    G: Fn(Decimal) -> String,
{
    ReportDailyBar {
        label: SharedString::from(label),
        progress: progress_of(cur, max),
        value_display: SharedString::from(if cur > Decimal::ZERO { fmt(cur) } else { String::new() }),
        bar_color: color,
        highlight,
        previous_progress: progress_of(prev, max),
        previous_display: SharedString::from(if prev > Decimal::ZERO { fmt(prev) } else { String::new() }),
    }
}

/// Fração `v / max` para a altura da barra (0.0 quando não há escala).
pub(crate) fn progress_of(v: Decimal, max: Decimal) -> f32 {
    if max > Decimal::ZERO {
        (v / max).to_f64().unwrap_or(0.0) as f32
    } else {
        0.0
    }
}

/// Mesma fração para séries de CONTAGEM (pedidos por hora / por canal).
pub(crate) fn count_progress(v: u32, max: u32) -> f32 {
    if max > 0 {
        (v as f64 / max as f64) as f32
    } else {
        0.0
    }
}

/// Tempo médio de preparo formatado (`None` = sem base para calcular).
pub(crate) fn prep_value(minutes: Option<f64>) -> String {
    match minutes {
        Some(m) if m < 60.0 => format!("{:.0} min", m),
        Some(m) => format!("{}h {:02}min", (m / 60.0) as u32, (m % 60.0) as u32),
        None => String::new(),
    }
}

/// Subtítulo do KPI de preparo: quantos pedidos formaram a base.
pub(crate) fn prep_sub(completed: usize) -> String {
    if completed == 0 {
        "Nenhum pedido completado".into()
    } else {
        format!("Base: {} pedidos completados", completed)
    }
}

/// Igual ao `money_compact` do dashboard: sem prefixo "R$ ",
/// 2 casas com vírgula em pt-BR. Usado nos tooltips dos candles
/// para não estourar a largura da pílula.
pub(crate) fn money_plain(v: Decimal) -> String {
    // 0,005 é o limiar de "meio centavo" — abaixo disso não há o que
    // mostrar na pílula.
    if v.abs() < Decimal::new(5, 3) {
        String::new()
    } else {
        format!("{:.2}", v).replace('.', ",")
    }
}

pub(crate) fn month_short(m: u32) -> &'static str {
    match m {
        1 => "Jan", 2 => "Fev", 3 => "Mar", 4 => "Abr",
        5 => "Mai", 6 => "Jun", 7 => "Jul", 8 => "Ago",
        9 => "Set", 10 => "Out", 11 => "Nov", 12 => "Dez",
        _ => "—",
    }
}

pub(crate) fn weekday_short(d: NaiveDate) -> &'static str {
    match d.weekday() {
        chrono::Weekday::Mon => "Seg",
        chrono::Weekday::Tue => "Ter",
        chrono::Weekday::Wed => "Qua",
        chrono::Weekday::Thu => "Qui",
        chrono::Weekday::Fri => "Sex",
        chrono::Weekday::Sat => "Sáb",
        chrono::Weekday::Sun => "Dom",
    }
}

pub(crate) fn color_for(s: &str) -> Color {
    let palette = [
        (0xE5, 0x39, 0x35),
        (0xF9, 0xA8, 0x25),
        (0x43, 0xA0, 0x47),
        (0x1E, 0x88, 0xE5),
        (0x8E, 0x24, 0xAA),
        (0xFB, 0x8C, 0x00),
        (0x00, 0x89, 0x7B),
        (0xC2, 0x18, 0x5B),
    ];
    let mut h: u32 = 0;
    for b in s.as_bytes() {
        h = h.wrapping_mul(31).wrapping_add(*b as u32);
    }
    let (r, g, b) = palette[(h as usize) % palette.len()];
    Color::from_rgb_u8(r, g, b)
}
