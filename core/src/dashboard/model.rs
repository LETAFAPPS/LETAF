//! Tipos do analytics de dashboard (domínio puro).
//!
//! São NÚMEROS e chaves de domínio (datas, horas, nomes) — sem nada de
//! apresentação (rótulos pt-BR, cores, SVG, tipos de UI vivem no frontend, §3).

use chrono::NaiveDate;
use rust_decimal::Decimal;

/// Período selecionado no filtro do dashboard.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DashboardPeriod {
    Today,
    Week,
    Month,
}

impl DashboardPeriod {
    /// Converte o valor vindo da UI (`"today"`/`"week"`/`"month"`); qualquer
    /// outro cai em `Week` (padrão), como no comportamento anterior.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "today" => Self::Today,
            "month" => Self::Month,
            _ => Self::Week,
        }
    }
}

/// Um ponto de série temporal já agregado. O RÓTULO é responsabilidade da UI:
/// - `hour = Some(h)` → bucket por hora (períodos por hora); rótulo `"{h}h"`.
/// - `hour = None`    → bucket por dia; a UI rotula a partir de `date`
///   (nome do dia da semana ou `date.day()`).
pub struct TimeBucket {
    pub date: NaiveDate,
    pub hour: Option<u32>,
    pub revenue: Decimal,
}

/// Ponto do comparativo (período atual vs período anterior equivalente).
pub struct ComparePoint {
    pub date: NaiveDate,
    pub hour: Option<u32>,
    pub current: Decimal,
    pub previous: Decimal,
}

/// Produto no ranking de mais vendidos (por receita no período).
pub struct TopProduct {
    pub name: String,
    pub revenue: Decimal,
    pub quantity: f64,
}

/// Receita por forma de pagamento no período (as 4 formas do donut).
pub struct PaymentBreakdown {
    pub pix: Decimal,
    pub credit: Decimal,
    pub debit: Decimal,
    pub cash: Decimal,
}

/// Resultado completo do analytics — tudo que o dashboard precisa exibir,
/// como dados brutos. A UI apenas formata/desenha.
pub struct DashboardMetrics {
    // KPIs de "hoje"
    pub revenue_today: Decimal,
    pub revenue_today_delta: Option<f64>, // vs mesmo dia da semana anterior (%)
    pub orders_today: u32,
    pub orders_today_delta: Option<f64>,
    pub avg_ticket_today: Decimal,
    pub avg_ticket_delta: Option<f64>, // vs média de ticket dos últimos 7 dias

    // Séries / comparativos
    pub sales_week: Vec<TimeBucket>, // segunda..domingo da semana corrente (7)
    pub compare: Vec<ComparePoint>,  // buckets conforme o período
    pub period_series: Vec<TimeBucket>, // 7 buckets do hero, conforme o período

    // Agregados do PERÍODO selecionado (hero)
    pub period_revenue: Decimal,
    pub period_revenue_delta: Option<f64>,
    pub period_orders: u32,
    pub period_ticket: Decimal,
    pub period_best_day: Option<NaiveDate>, // dia de maior receita na janela (>0)

    // Rankings
    pub top_products: Vec<TopProduct>, // top 5 por receita
    pub payments: PaymentBreakdown,
}
