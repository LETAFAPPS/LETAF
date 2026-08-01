use std::collections::HashMap;

use slint::{Color, Image, ModelRc, SharedString, VecModel};
use uuid::Uuid;

use letaf_core::category::model::Category;
use letaf_core::customer::model::Customer;
use letaf_core::order::model::Order;
use letaf_core::product::model::Product;
use letaf_core::report::{self, ReportPeriod};

use crate::{
    MainWindow, ReportCustomerRow, ReportDailyBar, ReportDreLine, ReportHBar, ReportHourlyBar,
    ReportKpi, ReportNewVsReturning, ReportOption, ReportProductRow,
};

use super::state::{Granularity, ReportState};
use super::helpers::ChartWindow;
use super::sections::{fill_customers, fill_financial, fill_orders, fill_products};
use super::helpers::opt;
use super::super::image::decode_pixel_buffer;
use slint::ComponentHandle;
use crate::ReportsState;

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
    // Fiados em ABERTO de qualquer data — o "a receber" não tem recorte de
    // período, então não sai de `orders`, que agora traz só a janela.
    fiado_aberto: &[Order],
    products: &[Product],
    categories: &[Category],
    customers: &[Customer],
) -> Snapshot {
    let today = letaf_core::tz::today();
    // A janela do período (e a anterior equivalente) vem do core — a
    // mesma regra do dashboard, sem reimplementar calendário na UI
    // (§14: não duplicar lógica).
    let period = ReportPeriod::from_str(&s.period);
    let win = report::period_window(today, period);
    let (period_label, granularity) = period_style(period);
    let chart_window = ChartWindow {
        start: win.start,
        end: win.end,
        today,
        prev_start: win.prev_start,
        granularity,
    };

    // Recortes de pedidos (soft delete, fuso da loja e cancelados) são
    // regra de domínio — vêm do core.
    let in_window = report::in_window(orders, win.start, win.end);
    let valid = report::non_cancelled(&in_window);
    let prev_in_window = report::in_window(orders, win.prev_start, win.prev_end);
    let prev_valid = report::non_cancelled(&prev_in_window);

    // Índices usados só para APRESENTAÇÃO (imagens e fotos).
    let product_by_id: HashMap<Uuid, &Product> = products.iter().map(|p| (p.base.id, p)).collect();
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

    // Cada sub-relatório: métricas do core → builder de apresentação.
    match s.kind.as_str() {
        "financial" => fill_financial(
            &mut snap,
            &report::financial(&valid, products),
            report::outstanding_fiado(fiado_aberto),
            &valid,
            &prev_valid,
            chart_window,
            win.days,
        ),
        "orders" => fill_orders(
            &mut snap,
            &report::orders(&in_window, &valid, &prev_valid),
            &in_window,
            &prev_in_window,
            chart_window,
        ),
        "products" => fill_products(
            &mut snap,
            &report::products(&valid, products, categories),
            &product_by_id,
        ),
        "customers" => fill_customers(
            &mut snap,
            &report::customers(&valid, orders, win.start, win.end),
            &customer_by_id,
        ),
        _ => {}
    }

    snap
}

/// Rótulo do período e granularidade do gráfico — pura apresentação.
fn period_style(period: ReportPeriod) -> (&'static str, Granularity) {
    match period {
        ReportPeriod::Daily => ("Dia Corrente", Granularity::Hourly),
        ReportPeriod::Weekly => ("Semana Corrente", Granularity::Daily),
        ReportPeriod::Monthly => ("Mês Corrente", Granularity::Daily),
        ReportPeriod::Yearly => ("Ano Corrente", Granularity::Monthly),
    }
}

pub(crate) fn apply_to_ui(ui: &MainWindow, s: &Snapshot) {
    ui.global::<ReportsState>().set_report_types(ModelRc::new(VecModel::from(s.types.clone())));
    ui.global::<ReportsState>().set_report_periods(ModelRc::new(VecModel::from(s.periods.clone())));
    ui.global::<ReportsState>().set_report_kpis(ModelRc::new(VecModel::from(s.kpis.clone())));
    ui.global::<ReportsState>().set_report_active_type(SharedString::from(s.active_type.clone()));
    ui.global::<ReportsState>().set_report_header_title(SharedString::from(s.header_title.clone()));
    ui.global::<ReportsState>().set_report_header_subtitle(SharedString::from(s.header_subtitle.clone()));
    ui.global::<ReportsState>().set_report_chart_title(SharedString::from(s.chart_title.clone()));
    ui.global::<ReportsState>().set_report_chart_subtitle(SharedString::from(s.chart_subtitle.clone()));
    ui.global::<ReportsState>().set_report_daily_bars(ModelRc::new(VecModel::from(s.daily_bars.clone())));
    ui.global::<ReportsState>().set_report_dre_lines(ModelRc::new(VecModel::from(s.dre_lines.clone())));
    ui.global::<ReportsState>().set_report_method_bars(ModelRc::new(VecModel::from(s.method_bars.clone())));
    ui.global::<ReportsState>().set_report_method_total(SharedString::from(s.method_total.clone()));
    ui.global::<ReportsState>().set_report_orders_bars(ModelRc::new(VecModel::from(s.orders_bars.clone())));
    ui.global::<ReportsState>().set_report_channel_bars(ModelRc::new(VecModel::from(s.channel_bars.clone())));
    ui.global::<ReportsState>().set_report_hourly_bars(ModelRc::new(VecModel::from(s.hourly_bars.clone())));
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
    ui.global::<ReportsState>().set_report_top_products(ModelRc::new(VecModel::from(product_rows)));
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
    ui.global::<ReportsState>().set_report_top_customers(ModelRc::new(VecModel::from(customer_rows)));
    ui.global::<ReportsState>().set_report_new_vs_ret(s.new_vs_ret.clone());
}
