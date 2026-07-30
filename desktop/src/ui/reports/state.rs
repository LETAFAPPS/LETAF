use std::sync::Arc;


use letaf_core::category::model::Category;
use letaf_core::customer::model::Customer;
use letaf_core::order::model::Order;
use letaf_core::product::model::Product;



// ── Estado ──────────────────────────────────────────────────────

#[derive(Clone)]
pub(crate) struct ReportState {
    /// "financial" | "orders" | "products" | "customers"
    pub(crate) kind: String,
    /// "7d" | "30d" | "month"
    pub(crate) period: String,
}

impl Default for ReportState {
    fn default() -> Self {
        Self {
            kind: "financial".into(),
            period: "weekly".into(),
        }
    }
}

/// Granularidade do eixo X dos gráficos diários.
#[derive(Clone, Copy)]
pub(crate) enum Granularity {
    /// 1 ponto por hora do dia corrente (24 pontos).
    Hourly,
    /// 1 ponto por dia entre start..end.
    Daily,
    /// 1 ponto por mês do ano corrente (12 pontos).
    Monthly,
}

pub(crate) type Shared<T> = Arc<std::sync::Mutex<T>>;

pub(crate) struct Caches {
    pub(crate) orders: Shared<Vec<Order>>,
    pub(crate) products: Shared<Vec<Product>>,
    pub(crate) categories: Shared<Vec<Category>>,
    pub(crate) customers: Shared<Vec<Customer>>,
}

impl Clone for Caches {
    fn clone(&self) -> Self {
        Self {
            orders: self.orders.clone(),
            products: self.products.clone(),
            categories: self.categories.clone(),
            customers: self.customers.clone(),
        }
    }
}

