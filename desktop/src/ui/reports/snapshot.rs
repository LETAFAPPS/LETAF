use std::collections::HashMap;

use chrono::{Datelike, Duration, Local, NaiveDate};
use slint::{Color, Image, ModelRc, SharedString, VecModel};
use uuid::Uuid;

use letaf_core::category::model::Category;
use letaf_core::customer::model::Customer;
use letaf_core::order::model::{Order, OrderStatus};
use letaf_core::product::model::Product;

use crate::{
    MainWindow, ReportCustomerRow, ReportDailyBar, ReportDreLine, ReportHBar, ReportHourlyBar,
    ReportKpi, ReportNewVsReturning, ReportOption, ReportProductRow,
};

use super::state::{Granularity, ReportState};
use super::sections::{fill_customers, fill_financial, fill_orders, fill_products};
use super::helpers::opt;
use super::super::image::decode_pixel_buffer;

// ── Snapshot ────────────────────────────────────────────────────

#[derive(Clone)]
pub(crate) struct TopProductRaw {
    pub(crate) rank: i32,
    pub(crate) name: String,
    pub(crate) category: String,
    pub(crate) qty_display: String,
    pub(crate) revenue_display: String,
    pub(crate) progress: f32,
    pub(crate) swatch_color: Color,
    pub(crate) image_b64: Option<String>,
}

/// Versão Send-safe do Top Cliente — `slint::Image` é construída
/// no event loop (mesma técnica de [`TopProductRaw`]).
#[derive(Clone)]
pub(crate) struct TopCustomerRaw {
    pub(crate) initial: String,
    pub(crate) name: String,
    pub(crate) orders_display: String,
    pub(crate) revenue_display: String,
    pub(crate) progress: f32,
    pub(crate) is_vip: bool,
    pub(crate) initial_color: Color,
    pub(crate) photo_b64: Option<String>,
}

pub(crate) struct Snapshot {
    pub(crate) types: Vec<ReportOption>,
    pub(crate) periods: Vec<ReportOption>,
    pub(crate) kpis: Vec<ReportKpi>,
    pub(crate) active_type: String,
    pub(crate) header_title: String,
    pub(crate) header_subtitle: String,
    /// Título e subtítulo do gráfico principal — variam por
    /// combinação (sub-relatório × período).
    pub(crate) chart_title: String,
    pub(crate) chart_subtitle: String,
    pub(crate) daily_bars: Vec<ReportDailyBar>,
    pub(crate) dre_lines: Vec<ReportDreLine>,
    pub(crate) method_bars: Vec<ReportHBar>,
    pub(crate) method_total: String,
    pub(crate) orders_bars: Vec<ReportDailyBar>,
    pub(crate) channel_bars: Vec<ReportHBar>,
    pub(crate) hourly_bars: Vec<ReportHourlyBar>,
    /// Versão "raw" do Top Produtos — guarda o b64 da imagem em vez
    /// de `slint::Image` (que não é Send). O `apply_to_ui` decodifica
    /// dentro do event loop e produz o `ReportProductRow`.
    pub(crate) top_products: Vec<TopProductRaw>,
    pub(crate) top_customers: Vec<TopCustomerRaw>,
    pub(crate) new_vs_ret: ReportNewVsReturning,
}

pub(crate) fn build_snapshot(
    s: &ReportState,
    orders: &[Order],
    products: &[Product],
    categories: &[Category],
    customers: &[Customer],
) -> Snapshot {
    let today = Local::now().date_naive();
    // weekly  = semana corrente (Seg → Dom), igual ao dashboard
    // monthly = mês corrente (dia 1 até último dia do mês)
    // yearly  = ano corrente (Jan a Dez do ano em curso, agregado por mês)
    let (start, end, period_label, period_days, granularity) = match s.period.as_str() {
        "daily" => (today, today, "Dia Corrente", 1, Granularity::Hourly),
        "monthly" => {
            let first = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap_or(today);
            // Último dia do mês = (1º do mês seguinte) - 1 dia. Trata
            // dezembro virando para janeiro do ano seguinte.
            let next_month_first = if today.month() == 12 {
                NaiveDate::from_ymd_opt(today.year() + 1, 1, 1)
            } else {
                NaiveDate::from_ymd_opt(today.year(), today.month() + 1, 1)
            };
            let last = next_month_first
                .and_then(|d| d.pred_opt())
                .unwrap_or(today);
            // `period_days` = dias DECORRIDOS (1 .. hoje), usado nos
            // subtítulos dos KPIs. O gráfico ocupa o mês inteiro.
            let days_in_period = (today - first).num_days() + 1;
            (first, last, "Mês Corrente", days_in_period.max(1), Granularity::Daily)
        }
        "yearly" => {
            let first = NaiveDate::from_ymd_opt(today.year(), 1, 1).unwrap_or(today);
            (first, today, "Ano Corrente", 365, Granularity::Monthly)
        }
        _ => {
            // Segunda-feira desta semana → domingo desta semana.
            let monday = today - Duration::days(today.weekday().num_days_from_monday() as i64);
            let sunday = monday + Duration::days(6);
            (monday, sunday, "Semana Corrente", 7, Granularity::Daily)
        }
    };

    let in_window: Vec<&Order> = orders
        .iter()
        .filter(|o| o.base.deleted_at.is_none())
        .filter(|o| {
            let d = o.base.created_at.date();
            d >= start && d <= end
        })
        .collect();
    let valid: Vec<&Order> = in_window
        .iter()
        .copied()
        .filter(|o| o.status != OrderStatus::Cancelled)
        .collect();

    let product_by_id: HashMap<Uuid, &Product> = products.iter().map(|p| (p.base.id, p)).collect();
    let category_by_id: HashMap<Uuid, &Category> = categories.iter().map(|c| (c.base.id, c)).collect();
    let customer_by_id: HashMap<Uuid, &Customer> = customers.iter().map(|c| (c.base.id, c)).collect();

    let types = vec![
        opt("financial", "Financeiro", s.kind == "financial"),
        opt("orders", "Pedidos", s.kind == "orders"),
        opt("products", "Produtos", s.kind == "products"),
        opt("customers", "Clientes", s.kind == "customers"),
    ];
    let periods = vec![
        opt("daily", "Diário", s.period == "daily"),
        opt("weekly", "Semanal", s.period == "weekly"),
        opt("monthly", "Mensal", s.period == "monthly"),
        opt("yearly", "Anual", s.period == "yearly"),
    ];

    let type_label = match s.kind.as_str() {
        "orders" => "Pedidos",
        "products" => "Produtos",
        "customers" => "Clientes",
        _ => "Financeiro",
    };
    // `header_title` alimenta o PILL DE STATUS ("Tipo · Período").
    let header_title = format!("{} · {}", type_label, period_label);
    let header_subtitle = String::new();

    // Título/subtítulo do gráfico — dependem de (sub-relatório,
    // período). Para sub-relatórios que não exibem gráfico (Produtos,
    // Clientes), os valores ficam vazios.
    let (chart_title, chart_subtitle) = match (s.kind.as_str(), s.period.as_str()) {
        ("financial", "daily") => ("Receita do Dia", "Faturamento por hora"),
        ("financial", "weekly") => ("Receita Semanal", "Faturamento por dia"),
        ("financial", "monthly") => ("Receita Mensal", "Faturamento por dia"),
        ("financial", "yearly") => ("Receita Anual", "Faturamento por mês"),
        ("orders", "daily") => ("Pedidos do Dia", "Volume por hora · dia corrente"),
        ("orders", "weekly") => ("Pedidos da Semana", "Volume diário · semana corrente"),
        ("orders", "monthly") => ("Pedidos do Mês", "Volume diário · mês corrente"),
        ("orders", "yearly") => ("Pedidos do Ano", "Volume mensal · ano corrente"),
        _ => ("", ""),
    };

    // Defaults (cada branch sobrescreve só o necessário).
    let mut snap = Snapshot {
        types,
        periods,
        kpis: Vec::new(),
        active_type: s.kind.clone(),
        header_title,
        header_subtitle,
        chart_title: chart_title.to_string(),
        chart_subtitle: chart_subtitle.to_string(),
        daily_bars: Vec::new(),
        dre_lines: Vec::new(),
        method_bars: Vec::new(),
        method_total: String::new(),
        orders_bars: Vec::new(),
        channel_bars: Vec::new(),
        hourly_bars: Vec::new(),
        top_products: Vec::new(),
        top_customers: Vec::new(),
        new_vs_ret: ReportNewVsReturning {
            new_count: 0,
            new_pct: SharedString::from("0%"),
            new_progress: 0.0,
            returning_count: 0,
            returning_pct: SharedString::from("0%"),
            returning_progress: 0.0,
        },
    };

    match s.kind.as_str() {
        "financial" => fill_financial(&mut snap, &in_window, &valid, &product_by_id, start, end, period_days, today, granularity),
        "orders" => fill_orders(&mut snap, &in_window, &valid, start, end, today, granularity),
        "products" => fill_products(&mut snap, &valid, &product_by_id, &category_by_id),
        "customers" => fill_customers(&mut snap, &valid, orders, &customer_by_id, start, end),
        _ => {}
    }

    snap
}

pub(crate) fn apply_to_ui(ui: &MainWindow, s: &Snapshot) {
    ui.set_report_types(ModelRc::new(VecModel::from(s.types.clone())));
    ui.set_report_periods(ModelRc::new(VecModel::from(s.periods.clone())));
    ui.set_report_kpis(ModelRc::new(VecModel::from(s.kpis.clone())));
    ui.set_report_active_type(SharedString::from(s.active_type.clone()));
    ui.set_report_header_title(SharedString::from(s.header_title.clone()));
    ui.set_report_header_subtitle(SharedString::from(s.header_subtitle.clone()));
    ui.set_report_chart_title(SharedString::from(s.chart_title.clone()));
    ui.set_report_chart_subtitle(SharedString::from(s.chart_subtitle.clone()));
    ui.set_report_daily_bars(ModelRc::new(VecModel::from(s.daily_bars.clone())));
    ui.set_report_dre_lines(ModelRc::new(VecModel::from(s.dre_lines.clone())));
    ui.set_report_method_bars(ModelRc::new(VecModel::from(s.method_bars.clone())));
    ui.set_report_method_total(SharedString::from(s.method_total.clone()));
    ui.set_report_orders_bars(ModelRc::new(VecModel::from(s.orders_bars.clone())));
    ui.set_report_channel_bars(ModelRc::new(VecModel::from(s.channel_bars.clone())));
    ui.set_report_hourly_bars(ModelRc::new(VecModel::from(s.hourly_bars.clone())));
    // Decodifica miniaturas no event loop (Image não é Send).
    let product_rows: Vec<ReportProductRow> = s
        .top_products
        .iter()
        .map(|p| {
            let (img, has_img) = p
                .image_b64
                .as_deref()
                .and_then(decode_pixel_buffer)
                .map(|buf| (Image::from_rgba8(buf), true))
                .unwrap_or((Image::default(), false));
            ReportProductRow {
                rank: p.rank,
                name: SharedString::from(p.name.clone()),
                category: SharedString::from(p.category.clone()),
                qty_display: SharedString::from(p.qty_display.clone()),
                revenue_display: SharedString::from(p.revenue_display.clone()),
                progress: p.progress,
                swatch_color: p.swatch_color,
                product_image: img,
                has_image: has_img,
            }
        })
        .collect();
    ui.set_report_top_products(ModelRc::new(VecModel::from(product_rows)));
    // Mesma técnica para clientes: decodifica foto no event loop.
    let customer_rows: Vec<ReportCustomerRow> = s
        .top_customers
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let (img, has_photo) = c
                .photo_b64
                .as_deref()
                .and_then(decode_pixel_buffer)
                .map(|buf| (Image::from_rgba8(buf), true))
                .unwrap_or((Image::default(), false));
            ReportCustomerRow {
                rank: (i + 1) as i32,
                initial: SharedString::from(c.initial.clone()),
                name: SharedString::from(c.name.clone()),
                orders_display: SharedString::from(c.orders_display.clone()),
                revenue_display: SharedString::from(c.revenue_display.clone()),
                progress: c.progress,
                is_vip: c.is_vip,
                initial_color: c.initial_color,
                profile_picture: img,
                has_photo,
            }
        })
        .collect();
    ui.set_report_top_customers(ModelRc::new(VecModel::from(customer_rows)));
    ui.set_report_new_vs_ret(s.new_vs_ret.clone());
}

