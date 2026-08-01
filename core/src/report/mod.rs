//! Analytics de relatórios (domínio puro, §1/§3): DRE simplificada,
//! agregações de pedidos, produtos e clientes e a janela de período.
//!
//! Sem persistência e sem apresentação: a UI consome estes números e
//! cuida apenas de formatação, cores e geometria dos gráficos.

pub mod model;
pub mod service;

pub use model::{
    ChannelCounts, CustomerAggregate, CustomerMetrics, FinancialMetrics, MethodRevenue,
    OrdersMetrics, ProductAggregate, ProductMetrics, ReportPeriod, ReportWindow,
};
pub use service::{
    customers, financial, in_window, non_cancelled, orders, outstanding_fiado, period_window,
    products,
};
