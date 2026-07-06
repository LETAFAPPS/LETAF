
use chrono::{Datelike, Duration, NaiveDate, Timelike};
use slint::{Color, SharedString};

use letaf_core::order::model::{Order, OrderStatus};

use crate::{
    ReportDailyBar, ReportDreLine,
    ReportKpi, ReportOption,
};

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

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_daily<F, G>(
    start: NaiveDate,
    end: NaiveDate,
    today: NaiveDate,
    orders: &[&Order],
    granularity: Granularity,
    per_order: F,
    fmt: G,
    color: Color,
) -> Vec<ReportDailyBar>
where
    F: Fn(&Order) -> f64,
    G: Fn(f64) -> String,
{
    match granularity {
        Granularity::Hourly => build_hourly_buckets(today, orders, per_order, fmt, color),
        Granularity::Daily => build_daily_buckets(start, end, today, orders, per_order, fmt, color),
        Granularity::Monthly => build_monthly_buckets(today.year(), today, orders, per_order, fmt, color),
    }
}

/// Buckets horários do DIA corrente (00h..23h). Rótulo a cada 3h pra
/// não poluir; barra destaca a hora atual.
pub(crate) fn build_hourly_buckets<F, G>(
    today: NaiveDate,
    orders: &[&Order],
    per_order: F,
    fmt: G,
    color: Color,
) -> Vec<ReportDailyBar>
where
    F: Fn(&Order) -> f64,
    G: Fn(f64) -> String,
{
    let now_hour = chrono::Local::now().hour();
    let mut totals = [0.0_f64; 24];
    for o in orders {
        if o.base.created_at.date() == today {
            let h = o.base.created_at.hour() as usize;
            if h < 24 { totals[h] += per_order(o); }
        }
    }
    let max = totals.iter().copied().fold(0.0_f64, f64::max);
    (0..24_usize)
        .map(|h| {
            let v = totals[h];
            let label = if h % 3 == 0 { format!("{:02}h", h) } else { String::new() };
            ReportDailyBar {
                label: SharedString::from(label),
                progress: if max > 0.0 { (v / max) as f32 } else { 0.0 },
                value_display: SharedString::from(if v > 0.0 { fmt(v) } else { String::new() }),
                bar_color: color,
                highlight: h as u32 == now_hour,
            }
        })
        .collect()
}

pub(crate) fn build_daily_buckets<F, G>(
    start: NaiveDate,
    end: NaiveDate,
    today: NaiveDate,
    orders: &[&Order],
    per_order: F,
    fmt: G,
    color: Color,
) -> Vec<ReportDailyBar>
where
    F: Fn(&Order) -> f64,
    G: Fn(f64) -> String,
{
    let span = (end - start).num_days() + 1;
    let mut values: Vec<(NaiveDate, f64)> = (0..span)
        .map(|i| (start + Duration::days(i), 0.0))
        .collect();
    for o in orders {
        let d = o.base.created_at.date();
        let idx = (d - start).num_days();
        if idx >= 0 {
            if let Some((_, v)) = values.get_mut(idx as usize) {
                *v += per_order(o);
            }
        }
    }
    let max = values.iter().map(|(_, v)| *v).fold(0.0_f64, f64::max);
    // Span ≤ 14 dias (semanal): weekday curto. Acima (mensal): TODOS
    // os números de dia do mês — o usuário pediu explicitamente.
    let use_weekday = span <= 14;
    values
        .into_iter()
        .map(|(d, v)| {
            let label = if use_weekday {
                weekday_short(d).to_string()
            } else {
                format!("{:02}", d.day())
            };
            ReportDailyBar {
                label: SharedString::from(label),
                progress: if max > 0.0 { (v / max) as f32 } else { 0.0 },
                value_display: SharedString::from(if v > 0.0 { fmt(v) } else { String::new() }),
                bar_color: color,
                highlight: d == today,
            }
        })
        .collect()
}

pub(crate) fn build_monthly_buckets<F, G>(
    year: i32,
    today: NaiveDate,
    orders: &[&Order],
    per_order: F,
    fmt: G,
    color: Color,
) -> Vec<ReportDailyBar>
where
    F: Fn(&Order) -> f64,
    G: Fn(f64) -> String,
{
    let mut totals = [0.0_f64; 12];
    for o in orders {
        let d = o.base.created_at.date();
        if d.year() == year {
            let m = d.month() as usize;
            if (1..=12).contains(&m) {
                totals[m - 1] += per_order(o);
            }
        }
    }
    let max = totals.iter().copied().fold(0.0_f64, f64::max);
    let today_month = today.month() as usize;
    (1..=12_usize)
        .map(|m| {
            let v = totals[m - 1];
            ReportDailyBar {
                label: SharedString::from(month_short(m as u32)),
                progress: if max > 0.0 { (v / max) as f32 } else { 0.0 },
                value_display: SharedString::from(if v > 0.0 { fmt(v) } else { String::new() }),
                bar_color: color,
                highlight: m == today_month && today.year() == year,
            }
        })
        .collect()
}

/// Tempo médio de "preparo" aproximado: `updated_at - created_at`
/// para pedidos `Ready` ou `Delivered`. O domínio ainda não tem
/// timestamps explícitos por transição de status (§6), então usamos
/// o `updated_at` como proxy do momento em que o pedido atingiu seu
/// status final. Filtra durações <= 5s (criação/finalização imediata
/// — provavelmente PDV concluído num único clique, sem fase de
/// preparo real) e > 6h (provavelmente edição manual posterior, não
/// é tempo de preparo).
pub(crate) fn avg_prep_minutes(orders: &[&Order]) -> Option<f64> {
    let mut sum = 0.0_f64;
    let mut count = 0_u32;
    for o in orders {
        if !matches!(o.status, OrderStatus::Ready | OrderStatus::Delivered) {
            continue;
        }
        let delta = (o.base.updated_at - o.base.created_at).num_seconds();
        if !(5..=6 * 3600).contains(&delta) { continue; }
        sum += delta as f64;
        count += 1;
    }
    if count == 0 { None } else { Some(sum / count as f64 / 60.0) }
}

pub(crate) fn avg_prep_value(orders: &[&Order]) -> String {
    match avg_prep_minutes(orders) {
        Some(m) if m < 60.0 => format!("{:.0} min", m),
        Some(m) => format!("{}h {:02}min", (m / 60.0) as u32, (m % 60.0) as u32),
        None => "".into(),
    }
}

pub(crate) fn avg_prep_sub(orders: &[&Order]) -> String {
    let n = orders
        .iter()
        .filter(|o| matches!(o.status, OrderStatus::Ready | OrderStatus::Delivered))
        .count();
    if n == 0 {
        "Nenhum pedido completado".into()
    } else {
        format!("Base: {} pedidos completados", n)
    }
}

/// Igual ao `money_compact` do dashboard: sem prefixo "R$ ",
/// 2 casas com vírgula em pt-BR. Usado nos tooltips dos candles
/// para não estourar a largura da pílula.
pub(crate) fn money_plain(v: f64) -> String {
    if v.abs() < 0.005 {
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
