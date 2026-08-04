use std::sync::Arc;


use letaf_core::category::model::Category;
use letaf_core::customer::model::Customer;
use letaf_core::order::model::Order;
use letaf_core::product::model::Product;
use letaf_core::product::stock_movement::StockMovement;



// ── Estado ──────────────────────────────────────────────────────

#[derive(Clone)]
pub(crate) struct ReportState {
    /// "financial" | "orders" | "products" | "customers"
    pub(crate) kind: String,
    /// "daily" | "weekly" | "monthly" | "yearly" — convertido em
    /// `letaf_core::report::ReportPeriod` na hora de montar o retrato.
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
    /// Pedidos da janela mais ampla que a tela pode pedir (ano atual + ano
    /// anterior) — não o histórico inteiro.
    pub(crate) orders: Shared<Vec<Order>>,
    /// Fiados em ABERTO de qualquer data: o "a receber" não tem recorte de
    /// período, então não cabe no cache acima.
    pub(crate) fiado_aberto: Shared<Vec<Order>>,
    pub(crate) products: Shared<Vec<Product>>,
    pub(crate) categories: Shared<Vec<Category>>,
    pub(crate) customers: Shared<Vec<Customer>>,
    /// Movimentos de estoque (ledger) da mesma janela ampla dos pedidos —
    /// recortados pelo período selecionado na hora de montar o extrato.
    pub(crate) stock_movements: Shared<Vec<StockMovement>>,
}

impl Clone for Caches {
    fn clone(&self) -> Self {
        Self {
            orders: self.orders.clone(),
            fiado_aberto: self.fiado_aberto.clone(),
            products: self.products.clone(),
            categories: self.categories.clone(),
            customers: self.customers.clone(),
            stock_movements: self.stock_movements.clone(),
        }
    }
}

