
use std::collections::HashMap;

use chrono::{Datelike, Duration, Local, NaiveDate, Timelike, Weekday};
use slint::{Color, ModelRc, SharedString, VecModel};

use letaf_core::order::model::{Order, OrderStatus};

use crate::format::money_br;
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

pub(crate) fn build_snapshot(
    orders: &[Order],
    sync_pending: i32,
    online: bool,
    syncing: bool,
    period: &str,
) -> Snapshot {
    let today = Local::now().date_naive();
    // Mesmo dia da semana anterior (ex.: se hoje é terça, terça passada).
    let same_day_last_week = today - Duration::days(7);

    // Filtra pedidos válidos (não cancelados, não removidos).
    let valid: Vec<&Order> = orders
        .iter()
        .filter(|o| o.base.deleted_at.is_none() && o.status != OrderStatus::Cancelled)
        .collect();

    // ── KPIs ────────────────────────────────────────────────
    let revenue_today: f64 = valid
        .iter()
        .filter(|o| o.base.created_at.date() == today)
        .map(|o| o.total)
        .sum();
    let revenue_baseline: f64 = valid
        .iter()
        .filter(|o| o.base.created_at.date() == same_day_last_week)
        .map(|o| o.total)
        .sum();
    let revenue_delta = pct_delta(revenue_today, revenue_baseline);

    let orders_today: Vec<&&Order> = valid
        .iter()
        .filter(|o| o.base.created_at.date() == today)
        .collect();
    let orders_today_count = orders_today.len();
    let pending_today = orders_today
        .iter()
        .filter(|o| o.status == OrderStatus::Pending)
        .count();
    let preparing_today = orders_today
        .iter()
        .filter(|o| o.status == OrderStatus::Preparing)
        .count();

    let avg_ticket_today = if orders_today_count > 0 {
        revenue_today / orders_today_count as f64
    } else {
        0.0
    };
    // Ticket médio dos últimos 7 dias (excluindo hoje) — base de comparação.
    let week_start = today - Duration::days(7);
    let last7: Vec<&&Order> = valid
        .iter()
        .filter(|o| {
            let d = o.base.created_at.date();
            d >= week_start && d < today
        })
        .collect();
    let last7_revenue: f64 = last7.iter().map(|o| o.total).sum();
    let last7_avg = if !last7.is_empty() {
        last7_revenue / last7.len() as f64
    } else {
        0.0
    };
    let ticket_delta = pct_delta(avg_ticket_today, last7_avg);
    // Pedidos hoje vs mesmo dia da semana passada (delta % pro KPI).
    let orders_baseline = valid
        .iter()
        .filter(|o| o.base.created_at.date() == same_day_last_week)
        .count();
    let orders_delta = pct_delta(orders_today_count as f64, orders_baseline as f64);
    let _ = (pending_today, preparing_today); // mantidos p/ uso futuro

    // Estado do sync: Aguardando (offline, vermelho) > Sincronizando
    // (ativo/pendências, laranja) > Sincronizado (verde).
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
            &money_br(revenue_today),
            &format_delta(revenue_delta, "vs semana"),
            delta_tone(revenue_delta),
        ),
        // [1] PEDIDOS HOJE — valor = contagem, sub = delta % (curto).
        kpi(
            "PEDIDOS HOJE",
            &orders_today_count.to_string(),
            &format_delta_short(orders_delta),
            delta_tone(orders_delta),
        ),
        // [2] TICKET MÉDIO — sub = delta % (curto).
        kpi(
            "TICKET MÉDIO",
            &money_br(avg_ticket_today),
            &format_delta_short(ticket_delta),
            delta_tone(ticket_delta),
        ),
        // [3] SINCRONIZAÇÃO — valor = status curto, sub = nº pendentes.
        kpi("SINCRONIZAÇÃO", sync_text, &sync_pending.to_string(), sync_tone),
    ];

    // ── Vendas da semana — segunda a domingo (fixo) ─────────
    // Pega a segunda-feira da semana CORRENTE e itera 7 dias.
    // Dias futuros (depois de hoje) entram com valor 0 — barra ausente.
    let monday_this_week = today
        - Duration::days(today.weekday().num_days_from_monday() as i64);
    let mut sales_bars: Vec<(NaiveDate, f64)> = Vec::with_capacity(7);
    for i in 0..7 {
        let d = monday_this_week + Duration::days(i);
        let v: f64 = valid
            .iter()
            .filter(|o| o.base.created_at.date() == d)
            .map(|o| o.total)
            .sum();
        sales_bars.push((d, v));
    }
    let max_bar = sales_bars
        .iter()
        .map(|(_, v)| *v)
        .fold(0.0_f64, f64::max);
    let sales_bar_rows: Vec<DashboardBarPoint> = sales_bars
        .into_iter()
        .map(|(d, v)| DashboardBarPoint {
            label: SharedString::from(weekday_short(d.weekday())),
            value_display: SharedString::from(money_compact(v)),
            progress: if max_bar > 0.0 { (v / max_bar) as f32 } else { 0.0 },
            highlight: d == today,
        })
        .collect();

    // ── Comparativo (período atual vs anterior) ─────────────
    // Buckets conforme o filtro: hoje→horas, semana→dias, mês→dias.
    let day_rev = |d: NaiveDate| -> f64 {
        valid.iter().filter(|o| o.base.created_at.date() == d).map(|o| o.total).sum()
    };
    let hour_rev = |d: NaiveDate, h: u32| -> f64 {
        valid
            .iter()
            .filter(|o| o.base.created_at.date() == d && o.base.created_at.hour() == h)
            .map(|o| o.total)
            .sum()
    };
    let mut compare: Vec<(String, f64, f64)> = Vec::new();
    let compare_label: &str;
    match period {
        "today" => {
            compare_label = "Hoje vs ontem";
            let yest = today - Duration::days(1);
            for h in 0..24u32 {
                compare.push((format!("{:02}h", h), hour_rev(today, h), hour_rev(yest, h)));
            }
        }
        "month" => {
            compare_label = "Este mês vs anterior";
            let first = today.with_day(1).unwrap_or(today);
            let prev_last = first - Duration::days(1);
            let prev_first = prev_last.with_day(1).unwrap_or(prev_last);
            let days = days_in_month(today.year(), today.month());
            let prev_days = days_in_month(prev_first.year(), prev_first.month());
            for d in 1..=days {
                let cur = first.with_day(d).unwrap_or(first);
                let prev = prev_first.with_day(d.min(prev_days)).unwrap_or(prev_first);
                compare.push((d.to_string(), day_rev(cur), day_rev(prev)));
            }
        }
        _ => {
            compare_label = "Esta semana vs anterior";
            for i in 0..7 {
                let cur = monday_this_week + Duration::days(i);
                let prev = cur - Duration::days(7);
                compare.push((weekday_short(cur.weekday()).to_string(), day_rev(cur), day_rev(prev)));
            }
        }
    }
    let max_cmp = compare
        .iter()
        .flat_map(|(_, c, p)| [*c, *p])
        .fold(0.0_f64, f64::max);
    let compare_points: Vec<DashboardComparePoint> = compare
        .into_iter()
        .map(|(lbl, c, p)| DashboardComparePoint {
            label: SharedString::from(lbl),
            current_progress: if max_cmp > 0.0 { (c / max_cmp) as f32 } else { 0.0 },
            previous_progress: if max_cmp > 0.0 { (p / max_cmp) as f32 } else { 0.0 },
            current_display: SharedString::from(money_compact(c)),
            previous_display: SharedString::from(money_compact(p)),
        })
        .collect();

    // ── Hero: agregados do PERÍODO selecionado ──────────────
    // Série do gráfico em 7 buckets, conforme o filtro: HOJE por faixa
    // de hora (08h–22h), SEMANA por dia (seg..dom), MÊS por faixa de dias.
    let mut series_labels: Vec<SharedString> = Vec::with_capacity(7);
    let week_daily: Vec<f64> = match period {
        "today" => (0..7)
            .map(|i| {
                let h0 = 8 + i * 2;
                series_labels.push(SharedString::from(format!("{}h", h0)));
                valid
                    .iter()
                    .filter(|o| {
                        let dt = o.base.created_at;
                        dt.date() == today
                            && (dt.hour() as i64) >= h0
                            && (dt.hour() as i64) < h0 + 2
                    })
                    .map(|o| o.total)
                    .sum()
            })
            .collect(),
        "month" => {
            let first = today.with_day(1).unwrap_or(today);
            let span = (((today.day() as i64) + 6) / 7).max(1);
            (0..7)
                .map(|i| {
                    let d0 = first + Duration::days(i * span);
                    let d1 = first + Duration::days((i + 1) * span);
                    series_labels.push(SharedString::from(format!("{}", d0.day())));
                    valid
                        .iter()
                        .filter(|o| {
                            let d = o.base.created_at.date();
                            d >= d0 && d < d1 && d <= today
                        })
                        .map(|o| o.total)
                        .sum()
                })
                .collect()
        }
        _ => (0..7)
            .map(|i| {
                let d = monday_this_week + Duration::days(i);
                series_labels.push(SharedString::from(weekday_short(d.weekday())));
                valid.iter().filter(|o| o.base.created_at.date() == d).map(|o| o.total).sum()
            })
            .collect(),
    };

    // Janela do período (start..hoje) + janela anterior equivalente.
    let win_start = match period {
        "today" => today,
        "month" => today.with_day(1).unwrap_or(today),
        _ => monday_this_week, // "week"
    };
    let in_win = |d: NaiveDate| d >= win_start && d <= today;
    let (prev_start, prev_end) = match period {
        "today" => (today - Duration::days(1), today - Duration::days(1)),
        "month" => {
            let first = today.with_day(1).unwrap_or(today);
            let prev_last = first - Duration::days(1);
            (prev_last.with_day(1).unwrap_or(prev_last), prev_last)
        }
        _ => (monday_this_week - Duration::days(7), monday_this_week - Duration::days(1)),
    };
    let win_label = match period {
        "today" => "RECEITA DO DIA",
        "month" => "RECEITA DO MÊS",
        _ => "RECEITA DA SEMANA",
    };

    let week_revenue_val: f64 = valid
        .iter()
        .filter(|o| in_win(o.base.created_at.date()))
        .map(|o| o.total)
        .sum();
    let prev_rev: f64 = valid
        .iter()
        .filter(|o| {
            let d = o.base.created_at.date();
            d >= prev_start && d <= prev_end
        })
        .map(|o| o.total)
        .sum();
    let week_delta = pct_delta(week_revenue_val, prev_rev);
    let week_orders_count = valid.iter().filter(|o| in_win(o.base.created_at.date())).count();
    let week_ticket_val = if week_orders_count > 0 {
        week_revenue_val / week_orders_count as f64
    } else {
        0.0
    };
    // Melhor dia: "Hoje" no período diário; senão o dia de maior receita.
    let week_best_day = if period == "today" {
        "Hoje".to_string()
    } else {
        let mut by_day: HashMap<NaiveDate, f64> = HashMap::new();
        for o in valid.iter().filter(|o| in_win(o.base.created_at.date())) {
            *by_day.entry(o.base.created_at.date()).or_insert(0.0) += o.total;
        }
        by_day
            .into_iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .filter(|(_, v)| *v > 0.0)
            .map(|(d, _)| weekday_full(d.weekday()).to_string())
            .unwrap_or_else(|| "".to_string())
    };
    let week_line_path = build_line_path(&week_daily);
    let week_area_path = build_area_path(&week_daily);
    // Pontos (x,y em fração 0..1 da área) p/ os marcadores + hover.
    let line_max = week_daily.iter().cloned().fold(0.0_f64, f64::max);
    let line_points: Vec<DashboardLinePoint> = week_daily
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let norm = if line_max > 0.0 { v / line_max } else { 0.0 };
            DashboardLinePoint {
                x: (i as f32) / 6.0,
                y: (0.9 - norm * 0.8) as f32, // espelha build_line_path (margem 10/10)
                value_display: SharedString::from(money_br(*v)),
                label: series_labels.get(i).cloned().unwrap_or_default(),
            }
        })
        .collect();
    let week_has_chart = week_daily.iter().any(|v| *v > 0.0001);

    // ── Top Produtos (semana) ───────────────────────────────
    let mut prod: HashMap<String, (f64, f64)> = HashMap::new(); // nome -> (receita, qtd)
    for o in valid.iter().filter(|o| in_win(o.base.created_at.date())) {
        for it in &o.items {
            let e = prod.entry(it.product_name.clone()).or_insert((0.0, 0.0));
            e.0 += it.subtotal;
            e.1 += it.quantity;
        }
    }
    let mut prod_vec: Vec<(String, f64, f64)> =
        prod.into_iter().map(|(n, (r, q))| (n, r, q)).collect();
    prod_vec.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    prod_vec.truncate(5);
    let top_max_rev = prod_vec.first().map(|x| x.1).unwrap_or(0.0);
    let top_products: Vec<DashboardTopProduct> = prod_vec
        .iter()
        .enumerate()
        .map(|(i, (n, r, q))| {
            let progress = if top_max_rev > 0.0 { (*r / top_max_rev) as f32 } else { 0.0 };
            DashboardTopProduct {
                rank: (i + 1) as i32,
                name: SharedString::from(n.as_str()),
                revenue_display: SharedString::from(money_br(*r)),
                sales_display: SharedString::from(format!("{} vendas", *q as i64)),
                progress,
                arc_commands: SharedString::from(donut_arc(0.0, progress as f64)),
            }
        })
        .collect();

    // ── Formas de Pagamento (período) ───────────────────────
    // Cores FIXAS por forma — iguais nos dois temas (vêm do snapshot,
    // não do tema): Pix azul, Crédito vermelho, Débito amarelo,
    // Dinheiro verde. Cartão é separado em Crédito e Débito.
    let (mut pix, mut credit, mut debit, mut cash) = (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64);
    for o in valid.iter().filter(|o| in_win(o.base.created_at.date())) {
        match o.payment_method.as_deref() {
            Some("pix") => pix += o.total,
            Some("credit") => credit += o.total,
            Some("debit") => debit += o.total,
            Some("cash") => cash += o.total,
            _ => {} // carteira / sem método — fora do donut
        }
    }
    let pay_total = pix + credit + debit + cash;
    let pal = [
        ("Pix", pix, Color::from_rgb_u8(0x1E, 0x88, 0xE5), "pay-pix"),
        ("Cartão Crédito", credit, Color::from_rgb_u8(0xE5, 0x39, 0x35), "pay-cartao-credito"),
        ("Cartão Débito", debit, Color::from_rgb_u8(0xF9, 0xA8, 0x25), "pay-cartao-debito"),
        ("Dinheiro", cash, Color::from_rgb_u8(0x2E, 0x7D, 0x32), "pay-dinheiro"),
    ];
    // Só inclui as formas com valor; se nada foi recebido, lista todas
    // zeradas (mostra os métodos aceitos).
    let has_payments = pay_total > 0.0;
    let mut acc = 0.0_f64;
    let payment_slices: Vec<DashboardPaymentSlice> = pal
        .iter()
        .filter(|(_, val, _, _)| !has_payments || *val > 0.0)
        .map(|(label, val, color, icon_key)| {
            let frac = if pay_total > 0.0 { val / pay_total } else { 0.0 };
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
        sales_bars: sales_bar_rows,
        compare_points,
        compare_label: compare_label.to_string(),
        week_revenue: money_br(week_revenue_val),
        week_revenue_label: win_label.to_string(),
        week_delta: format_delta(week_delta, match period {
            "today" => "vs dia anterior",
            "month" => "vs mês anterior",
            _ => "vs semana anterior",
        }),
        week_delta_tone: delta_tone(week_delta).to_string(),
        week_has_chart,
        week_orders: week_orders_count.to_string(),
        week_ticket: money_br(week_ticket_val),
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

// ── Helpers ──────────────────────────────────────────────────────

pub(crate) fn kpi(label: &str, value: &str, sub: &str, tone: &str) -> DashboardKpi {
    DashboardKpi {
        label: SharedString::from(label),
        value_display: SharedString::from(value),
        sub_text: SharedString::from(sub),
        sub_tone: SharedString::from(tone),
    }
}

pub(crate) fn pct_delta(current: f64, baseline: f64) -> Option<f64> {
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

fn days_in_month(year: i32, month: u32) -> u32 {
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
fn money_k(v: f64) -> String {
    if v >= 1000.0 {
        format!("R${:.1}k", v / 1000.0).replace('.', ",")
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
