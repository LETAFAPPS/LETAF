//! Tipos do analytics de relatórios (domínio puro).
//!
//! São NÚMEROS e chaves de domínio — sem nada de apresentação (rótulos
//! pt-BR, cores, ícones e geometria de gráfico vivem no frontend, §3).
//! Dinheiro é sempre `Decimal` (exato); quantidades continuam `f64`.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use uuid::Uuid;

/// Período selecionado no filtro da tela de Relatórios.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReportPeriod {
    /// Dia corrente.
    Daily,
    /// Semana corrente (segunda → domingo).
    Weekly,
    /// Mês corrente (dia 1 → último dia do mês).
    Monthly,
    /// Ano corrente (1º de janeiro → hoje).
    Yearly,
}

impl ReportPeriod {
    /// Converte o valor vindo da UI; qualquer outro cai em `Weekly`
    /// (padrão), como no comportamento anterior.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "daily" => Self::Daily,
            "monthly" => Self::Monthly,
            "yearly" => Self::Yearly,
            _ => Self::Weekly,
        }
    }
}

/// Janela do período atual + a janela ANTERIOR equivalente (mesmo
/// recorte deslocado), usada como base de todo o relatório.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReportWindow {
    pub start: NaiveDate,
    pub end: NaiveDate,
    pub prev_start: NaiveDate,
    pub prev_end: NaiveDate,
    /// Dias DECORRIDOS do período (base dos subtítulos dos KPIs) — no
    /// mês corrente conta do dia 1 até hoje, não o mês inteiro.
    pub days: i64,
}

/// Receita por forma de pagamento (as 4 formas exibidas no gauge).
/// Pedidos sem forma registrada ou pagos com carteira ficam de fora.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MethodRevenue {
    pub cash: Decimal,
    pub pix: Decimal,
    pub credit: Decimal,
    pub debit: Decimal,
}

impl MethodRevenue {
    /// Total das formas CONHECIDAS — é este o 100% do gauge.
    pub fn total(&self) -> Decimal {
        self.cash + self.pix + self.credit + self.debit
    }
}

/// DRE simplificada + KPIs do sub-relatório Financeiro.
///
/// Só há linha para o que existe no domínio hoje (receita e custo de
/// produtos); despesas/taxas/impostos entram quando virarem entidades.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FinancialMetrics {
    /// Receita bruta = soma dos totais dos pedidos válidos.
    pub revenue: Decimal,
    /// Custo dos produtos vendidos (`cost_price × quantidade`).
    pub cost: Decimal,
    /// Lucro líquido = receita − custo.
    pub net: Decimal,
    /// Margem líquida em % — ZERO quando não houve receita.
    pub margin_pct: Decimal,
    pub orders_count: usize,
    /// Ticket médio — ZERO quando não houve pedido.
    pub avg_ticket: Decimal,
    pub methods: MethodRevenue,
}

/// Contagem de pedidos por canal de venda.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChannelCounts {
    /// Balcão (PDV): pedido com forma de pagamento registrada.
    pub pdv: u32,
    pub delivery: u32,
    pub pickup: u32,
}

/// KPIs e séries do sub-relatório Pedidos.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OrdersMetrics {
    /// Todos os pedidos da janela (inclusive cancelados).
    pub total: usize,
    pub valid: usize,
    pub cancelled: usize,
    /// Taxa de cancelamento em % — ZERO sem pedidos.
    pub cancel_rate: Decimal,
    pub avg_ticket: Decimal,
    pub channels: ChannelCounts,
    /// Pedidos válidos por hora local (0..23) do período atual.
    pub by_hour: [u32; 24],
    /// Idem para o período anterior (mesma escala no gráfico).
    pub prev_by_hour: [u32; 24],
    /// Tempo médio de preparo em minutos; `None` sem base.
    pub avg_prep_minutes: Option<f64>,
    /// Pedidos completados (prontos/entregues) — base do tempo médio.
    pub completed_count: usize,
}

/// Agregado de um produto no período.
#[derive(Clone, Debug, PartialEq)]
pub struct ProductAggregate {
    pub product_id: Uuid,
    /// Nome atual do produto (cai no snapshot do item se o produto
    /// não existir mais).
    pub name: String,
    /// Categoria do produto (vazio quando não houver).
    pub category_name: String,
    pub quantity: f64,
    /// Receita atribuída ao produto (rateio do total do pedido pela
    /// participação do item no subtotal).
    pub revenue: Decimal,
    pub cost: Decimal,
}

/// KPIs e ranking do sub-relatório Produtos.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProductMetrics {
    pub total_units: f64,
    pub total_revenue: Decimal,
    pub total_cost: Decimal,
    /// Margem média ponderada em % — ZERO sem receita.
    pub margin_pct: Decimal,
    /// Ranking por receita (desc); empate → nome (determinístico).
    pub ranking: Vec<ProductAggregate>,
    pub top_by_quantity: Option<ProductAggregate>,
    pub top_by_revenue: Option<ProductAggregate>,
}

/// Agregado de um cliente considerando TODO o histórico (LTV).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CustomerAggregate {
    pub customer_id: Uuid,
    pub revenue: Decimal,
    pub orders: i64,
    /// Cliente VIP = LTV ≥ 2× o LTV médio.
    pub is_vip: bool,
}

/// KPIs e ranking do sub-relatório Clientes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CustomerMetrics {
    /// Clientes com pedido válido no período.
    pub active_count: usize,
    /// Ativos cujo PRIMEIRO pedido caiu dentro do período.
    pub new_count: i32,
    pub returning_count: i32,
    /// Recorrentes ÷ ativos em % — ZERO sem clientes ativos.
    pub return_rate: Decimal,
    pub avg_ltv: Decimal,
    /// Ranking por LTV (desc); empate → id (determinístico).
    pub ranking: Vec<CustomerAggregate>,
}
