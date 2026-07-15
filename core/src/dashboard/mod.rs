//! Analytics do dashboard (domínio puro, §3): agregação de métricas de vendas
//! sobre pedidos, sem persistência e sem apresentação. A UI consome
//! `DashboardMetrics` e cuida apenas de formatação, cores e geometria (SVG).

pub mod model;
pub mod service;

pub use model::{
    ComparePoint, DashboardMetrics, DashboardPeriod, PaymentBreakdown, TimeBucket, TopProduct,
};
pub use service::compute;
