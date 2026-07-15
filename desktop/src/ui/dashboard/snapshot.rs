
use chrono::{Datelike, Local, NaiveDate, Weekday};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use slint::{Color, ModelRc, SharedString, VecModel};

use letaf_core::dashboard::{self, DashboardPeriod, TimeBucket};
use letaf_core::order::model::Order;

/// Formata um `Decimal` (dinheiro exato do domínio) no padrão pt-BR.
fn money_br(v: Decimal) -> String {
    crate::format::money_br(v)
}

/// Converte `Decimal` para `f64` — usado SÓ na geometria dos gráficos
/// (normalização de barras/linhas), nunca para persistir dinheiro.
fn f(v: Decimal) -> f64 {
    v.to_f64().unwrap_or(0.0)
}

use crate::{
    DashboardBarPoint, DashboardComparePoint, DashboardKpi, DashboardLinePoint,
    DashboardPaymentSlice, DashboardTopProduct, MainWindow,
};

// ── Snapshot agregado (Send para invocar do event loop) ──────────

pub(crate) struct Snapshot {
    pub(crate) kpis: Vec<DashboardKpi>,
    pub(crate) sales_bars: Vec<DashboardBarPoint>,
    pub(crate) compare_points: Vec<DashboardComparePoint>,
    pub(crate) compare_label: String,
    // Hero "Receita da Semana"
    pub(crate) week_revenue: String,
    pub(crate) week_revenue_label: String,
    pub(crate) week_delta: String,
    pub(crate) week_delta_tone: String,
    pub(crate) week_has_chart: bool,
    pub(crate) week_orders: String,
    pub(crate) week_ticket: String,
    pub(crate) week_best_day: String,
    pub(crate) week_line_path: String,
    pub(crate) week_area_path: String,
    pub(crate) line_points: Vec<DashboardLinePoint>,
    pub(crate) subtitle: String,
    // Top Produtos + Formas de Pagamento
    pub(crate) top_products: Vec<DashboardTopProduct>,
    pub(crate) payment_slices: Vec<DashboardPaymentSlice>,
    pub(crate) payment_total: String,
}

/// Monta o snapshot de exibição do dashboard: chama o analytics do CORE
/// (`dashboard::compute`, toda a regra de negócio, §3) e apenas MAPEIA o
/// resultado para os tipos da UI — formatação pt-BR, cores e geometria (SVG).
pub(crate) fn build_snapshot(
    orders: &[Order],
    sync_pending: i32,
    online: bool,
    syncing: bool,
    period: &str,
) -> Snapshot {
    let today = Local::now().date_naive();
    let per = DashboardPeriod::from_str(period);
    let m = dashboard::compute(orders, today, per);

    // ── KPIs ────────────────────────────────────────────────
    // Estado do sync: Aguardando (offline) > Sincronizando > Sincronizado.
    let (sync_text, sync_tone) = if !online {
        ("Aguardando", "neg")
    } else if syncing || sync_pending > 0 {
        ("Sincronizando", "warn")
    } else {
        ("Sincronizado", "pos")
    };
    let kpis = vec![
        kpi(
            "RECEITA HOJE",
            &money_br(m.revenue_today),
            &format_delta(m.revenue_today_delta, "vs semana"),
            delta_tone(m.revenue_today_delta),
        ),
        kpi(
            "PEDIDOS HOJE",
            &m.orders_today.to_string(),
            &format_delta_short(m.orders_today_delta),
            delta_tone(m.orders_today_delta),
        ),
        kpi(
            "TICKET MÉDIO",
            &money_br(m.avg_ticket_today),
            &format_delta_short(m.avg_ticket_delta),
            delta_tone(m.avg_ticket_delta),
        ),
        kpi("SINCRONIZAÇÃO", sync_text, &sync_pending.to_string(), sync_tone),
    ];

    // ── Vendas da semana (barras seg..dom) ──────────────────
    let max_bar = m.sales_week.iter().map(|b| f(b.revenue)).fold(0.0_f64, f64::max);
    let sales_bars: Vec<DashboardBarPoint> = m
        .sales_week
        .iter()
        .map(|b| {
            let v = f(b.revenue);
            DashboardBarPoint {
                label: SharedString::from(weekday_short(b.date.weekday())),
                value_display: SharedString::from(money_compact(v)),
                progress: if max_bar > 0.0 { (v / max_bar) as f32 } else { 0.0 },
                highlight: b.date == today,
            }
        })
        .collect();

    // ── Comparativo (rótulo do bucket depende do período) ───
    let compare_label_of = |date: NaiveDate, hour: Option<u32>| -> String {
        match per {
            DashboardPeriod::Today => format!("{:02}h", hour.unwrap_or(0)),
            DashboardPeriod::Month => format!("{}", date.day()),
            DashboardPeriod::Week => weekday_short(date.weekday()).to_string(),
        }
    };
    let max_cmp = m
        .compare
        .iter()
        .flat_map(|c| [f(c.current), f(c.previous)])
        .fold(0.0_f64, f64::max);
    let compare_points: Vec<DashboardComparePoint> = m
        .compare
        .iter()
        .map(|c| {
            let (cur, prev) = (f(c.current), f(c.previous));
            DashboardComparePoint {
                label: SharedString::from(compare_label_of(c.date, c.hour)),
                current_progress: if max_cmp > 0.0 { (cur / max_cmp) as f32 } else { 0.0 },
                previous_progress: if max_cmp > 0.0 { (prev / max_cmp) as f32 } else { 0.0 },
                current_display: SharedString::from(money_compact(cur)),
                previous_display: SharedString::from(money_compact(prev)),
            }
        })
        .collect();
    let compare_label = match per {
        DashboardPeriod::Today => "Hoje vs ontem",
        DashboardPeriod::Month => "Este mês vs anterior",
        DashboardPeriod::Week => "Esta semana vs anterior",
    };

    // ── Hero: série do período (linha + área + marcadores) ──
    let series_label = |b: &TimeBucket| -> SharedString {
        match per {
            DashboardPeriod::Today => SharedString::from(format!("{}h", b.hour.unwrap_or(0))),
            DashboardPeriod::Month => SharedString::from(format!("{}", b.date.day())),
            DashboardPeriod::Week => SharedString::from(weekday_short(b.date.weekday())),
        }
    };
    let series: Vec<f64> = m.period_series.iter().map(|b| f(b.revenue)).collect();
    let week_line_path = build_line_path(&series);
    let week_area_path = build_area_path(&series);
    let line_max = series.iter().cloned().fold(0.0_f64, f64::max);
    let line_points: Vec<DashboardLinePoint> = m
        .period_series
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let norm = if line_max > 0.0 { f(b.revenue) / line_max } else { 0.0 };
            DashboardLinePoint {
                x: (i as f32) / 6.0,
                y: (0.9 - norm * 0.8) as f32, // espelha build_line_path (margem 10/10)
                value_display: SharedString::from(money_br(b.revenue)),
                label: series_label(b),
            }
        })
        .collect();
    let week_has_chart = series.iter().any(|v| *v > 0.0001);

    let win_label = match per {
        DashboardPeriod::Today => "RECEITA DO DIA",
        DashboardPeriod::Month => "RECEITA DO MÊS",
        DashboardPeriod::Week => "RECEITA DA SEMANA",
    };
    let delta_suffix = match per {
        DashboardPeriod::Today => "vs dia anterior",
        DashboardPeriod::Month => "vs mês anterior",
        DashboardPeriod::Week => "vs semana anterior",
    };
    // Melhor dia: "Hoje" no período diário; senão o dia de maior receita.
    let week_best_day = if per == DashboardPeriod::Today {
        "Hoje".to_string()
    } else {
        m.period_best_day
            .map(|d| weekday_full(d.weekday()).to_string())
            .unwrap_or_default()
    };

    // ── Top Produtos ────────────────────────────────────────
    let top_max_rev = m.top_products.first().map(|p| f(p.revenue)).unwrap_or(0.0);
    let top_products: Vec<DashboardTopProduct> = m
        .top_products
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let progress = if top_max_rev > 0.0 { (f(p.revenue) / top_max_rev) as f32 } else { 0.0 };
            DashboardTopProduct {
                rank: (i + 1) as i32,
                name: SharedString::from(p.name.as_str()),
                revenue_display: SharedString::from(money_br(p.revenue)),
                sales_display: SharedString::from(format!("{} vendas", p.quantity as i64)),
                progress,
                arc_commands: SharedString::from(donut_arc(0.0, progress as f64)),
            }
        })
        .collect();

    // ── Formas de Pagamento ─────────────────────────────────
    // Cores FIXAS por forma (iguais nos dois temas): Pix azul, Crédito
    // vermelho, Débito amarelo, Dinheiro verde.
    let pay = &m.payments;
    let pay_total = pay.pix + pay.credit + pay.debit + pay.cash;
    let pal = [
        ("Pix", pay.pix, Color::from_rgb_u8(0x1E, 0x88, 0xE5), "pay-pix"),
        ("Cartão Crédito", pay.credit, Color::from_rgb_u8(0xE5, 0x39, 0x35), "pay-cartao-credito"),
        ("Cartão Débito", pay.debit, Color::from_rgb_u8(0xF9, 0xA8, 0x25), "pay-cartao-debito"),
        ("Dinheiro", pay.cash, Color::from_rgb_u8(0x2E, 0x7D, 0x32), "pay-dinheiro"),
    ];
    // Só inclui as formas com valor; se nada foi recebido, lista todas
    // zeradas (mostra os métodos aceitos).
    let pay_total_f = f(pay_total);
    let has_payments = pay_total_f > 0.0;
    let mut acc = 0.0_f64;
    let payment_slices: Vec<DashboardPaymentSlice> = pal
        .iter()
        .filter(|(_, val, _, _)| !has_payments || f(*val) > 0.0)
        .map(|(label, val, color, icon_key)| {
            let frac = if pay_total_f > 0.0 { f(*val) / pay_total_f } else { 0.0 };
            let arc = donut_arc(acc, acc + frac);
            acc += frac;
            DashboardPaymentSlice {
                label: SharedString::from(*label),
                value_display: SharedString::from(money_br(*val)),
                pct_display: SharedString::from(format!("{}%", (frac * 100.0).round() as i64)),
                color: *color,
                icon_key: SharedString::from(*icon_key),
                arc_commands: SharedString::from(arc),
            }
        })
        .collect();

    Snapshot {
        kpis,
        sales_bars,
        compare_points,
        compare_label: compare_label.to_string(),
        week_revenue: money_br(m.period_revenue),
        week_revenue_label: win_label.to_string(),
        week_delta: format_delta(m.period_revenue_delta, delta_suffix),
        week_delta_tone: delta_tone(m.period_revenue_delta).to_string(),
        week_has_chart,
        week_orders: m.period_orders.to_string(),
        week_ticket: money_br(m.period_ticket),
        week_best_day,
        week_line_path,
        week_area_path,
        line_points,
        subtitle: subtitle_today(today),
        top_products,
        payment_slices,
        payment_total: money_k(pay_total),
    }
}

pub(crate) fn apply_to_ui(ui: &MainWindow, s: &Snapshot) {
    ui.set_dashboard_kpis(ModelRc::new(VecModel::from(s.kpis.clone())));
    ui.set_dashboard_sales_bars(ModelRc::new(VecModel::from(s.sales_bars.clone())));
    ui.set_dashboard_compare_points(ModelRc::new(VecModel::from(s.compare_points.clone())));
    ui.set_dashboard_compare_label(SharedString::from(s.compare_label.as_str()));
    ui.set_dashboard_week_revenue(SharedString::from(s.week_revenue.as_str()));
    ui.set_dashboard_week_revenue_label(SharedString::from(s.week_revenue_label.as_str()));
    ui.set_dashboard_week_delta(SharedString::from(s.week_delta.as_str()));
    ui.set_dashboard_week_delta_tone(SharedString::from(s.week_delta_tone.as_str()));
    ui.set_dashboard_week_has_chart(s.week_has_chart);
    ui.set_dashboard_week_orders(SharedString::from(s.week_orders.as_str()));
    ui.set_dashboard_week_ticket(SharedString::from(s.week_ticket.as_str()));
    ui.set_dashboard_week_best_day(SharedString::from(s.week_best_day.as_str()));
    ui.set_dashboard_week_line_path(SharedString::from(s.week_line_path.as_str()));
    ui.set_dashboard_week_area_path(SharedString::from(s.week_area_path.as_str()));
    ui.set_dashboard_line_points(ModelRc::new(VecModel::from(s.line_points.clone())));
    ui.set_dashboard_subtitle(SharedString::from(s.subtitle.as_str()));
    ui.set_dashboard_top_products(ModelRc::new(VecModel::from(s.top_products.clone())));
    ui.set_dashboard_payment_slices(ModelRc::new(VecModel::from(s.payment_slices.clone())));
    ui.set_dashboard_payment_total(SharedString::from(s.payment_total.as_str()));
}

// ── Helpers de apresentação ──────────────────────────────────────

pub(crate) fn kpi(label: &str, value: &str, sub: &str, tone: &str) -> DashboardKpi {
    DashboardKpi {
        label: SharedString::from(label),
        value_display: SharedString::from(value),
        sub_text: SharedString::from(sub),
        sub_tone: SharedString::from(tone),
    }
}

pub(crate) fn format_delta(delta: Option<f64>, suffix: &str) -> String {
    match delta {
        None => format!("Sem base · {}", suffix),
        Some(v) if v.abs() < 0.05 => format!("Estável · {}", suffix),
        Some(v) if v > 0.0 => format!("↑ +{:.1}% · {}", v, suffix).replace('.', ","),
        Some(v) => format!("↓ {:.1}% · {}", v, suffix).replace('.', ","),
    }
}

/// Versão curta (sem sufixo) pros KPIs pequenos — "↑ +8,2%".
/// Sem base de comparação → string vazia (o card fica sem o texto).
pub(crate) fn format_delta_short(delta: Option<f64>) -> String {
    match delta {
        None => String::new(),
        Some(v) if v.abs() < 0.05 => "Estável".to_string(),
        Some(v) if v > 0.0 => format!("↑ +{:.1}%", v).replace('.', ","),
        Some(v) => format!("↓ {:.1}%", v).replace('.', ","),
    }
}

pub(crate) fn delta_tone(delta: Option<f64>) -> &'static str {
    match delta {
        Some(v) if v > 0.05 => "pos",
        Some(v) if v < -0.05 => "neg",
        _ => "neutral",
    }
}

/// Formato compacto para tooltips dos candles: 2 casas decimais com
/// vírgula em vez de ponto (pt-BR). Vazio quando o valor é zero.
/// Ex.: 39.0 → "39,00", 1234.5 → "1234,50".
pub(crate) fn money_compact(v: f64) -> String {
    if v.abs() < 0.005 {
        String::new()
    } else {
        format!("{:.2}", v).replace('.', ",")
    }
}

pub(crate) fn weekday_short(w: Weekday) -> &'static str {
    match w {
        Weekday::Mon => "Seg",
        Weekday::Tue => "Ter",
        Weekday::Wed => "Qua",
        Weekday::Thu => "Qui",
        Weekday::Fri => "Sex",
        Weekday::Sat => "Sáb",
        Weekday::Sun => "Dom",
    }
}

pub(crate) fn weekday_full(w: Weekday) -> &'static str {
    match w {
        Weekday::Mon => "Segunda",
        Weekday::Tue => "Terça",
        Weekday::Wed => "Quarta",
        Weekday::Thu => "Quinta",
        Weekday::Fri => "Sexta",
        Weekday::Sat => "Sábado",
        Weekday::Sun => "Domingo",
    }
}

fn month_pt(m: u32) -> &'static str {
    match m {
        1 => "janeiro", 2 => "fevereiro", 3 => "março", 4 => "abril",
        5 => "maio", 6 => "junho", 7 => "julho", 8 => "agosto",
        9 => "setembro", 10 => "outubro", 11 => "novembro", _ => "dezembro",
    }
}

/// Subtítulo do header: "Terça, 22 de junho".
fn subtitle_today(d: NaiveDate) -> String {
    format!("{}, {} de {}", weekday_full(d.weekday()), d.day(), month_pt(d.month()))
}

/// Moeda compacta com sufixo "k" para o centro do donut. Ex.: 11770 → "R$11,8k".
fn money_k(v: Decimal) -> String {
    let vf = f(v);
    if vf >= 1000.0 {
        format!("R${:.1}k", vf / 1000.0).replace('.', ",")
    } else {
        money_br(v)
    }
}

/// Comandos SVG da linha de receita (viewbox 0 0 100 100). Normaliza
/// os 7 valores pelo maior; y invertido (SVG cresce pra baixo), com
/// margem no topo/base pra não colar nas bordas.
fn build_line_path(daily: &[f64]) -> String {
    if daily.len() < 2 {
        return String::new();
    }
    let max = daily.iter().cloned().fold(0.0_f64, f64::max);
    let n = daily.len();
    let mut s = String::new();
    for (i, v) in daily.iter().enumerate() {
        let x = i as f64 / (n - 1) as f64 * 100.0;
        let norm = if max > 0.0 { v / max } else { 0.0 };
        let y = 90.0 - norm * 80.0; // margem: topo 10, base 10
        if i == 0 {
            s.push_str(&format!("M {:.2} {:.2}", x, y));
        } else {
            s.push_str(&format!(" L {:.2} {:.2}", x, y));
        }
    }
    s
}

/// Mesma linha, mas fechada até a base — para o preenchimento (área)
/// sob a curva no hero. Viewbox 0 0 100 100.
fn build_area_path(daily: &[f64]) -> String {
    let line = build_line_path(daily);
    if line.is_empty() {
        return String::new();
    }
    format!("{} L 100 100 L 0 100 Z", line)
}

/// Arco de uma fatia do donut como comando SVG (viewbox 0 0 100 100,
/// centro 50,50, raio 38). `start`/`end` em fração (0..1) do círculo,
/// iniciando no topo (-90°) no sentido horário. Renderizado com stroke
/// (a espessura do stroke é o anel). Fatia vazia → string vazia.
fn donut_arc(start: f64, end: f64) -> String {
    let sweep = end - start;
    if sweep <= 0.0001 {
        return String::new();
    }
    let (cx, cy, r) = (50.0_f64, 50.0_f64, 38.0_f64);
    let point = |f: f64| {
        let a = (-90.0 + f * 360.0).to_radians();
        (cx + r * a.cos(), cy + r * a.sin())
    };
    if sweep >= 0.9999 {
        // Círculo cheio (um único método = 100%): dois semicírculos.
        let (x0, y0) = point(0.0);
        let (x1, y1) = point(0.5);
        return format!(
            "M {:.3} {:.3} A {r} {r} 0 1 1 {:.3} {:.3} A {r} {r} 0 1 1 {:.3} {:.3}",
            x0, y0, x1, y1, x0, y0
        );
    }
    let (x0, y0) = point(start);
    let (x1, y1) = point(end);
    let large = if sweep > 0.5 { 1 } else { 0 };
    format!("M {:.3} {:.3} A {r} {r} 0 {large} 1 {:.3} {:.3}", x0, y0, x1, y1)
}
