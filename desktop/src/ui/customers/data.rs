
use slint::SharedString;

use letaf_core::order::model::{Order, OrderStatus};



/// Pedido recente já formatado para o detalhe do cliente.
#[derive(Clone)]
pub(crate) struct RecentOrder {
    pub(crate) id: SharedString,
    pub(crate) number: SharedString,
    pub(crate) summary: SharedString,
    pub(crate) date: SharedString,
    pub(crate) status: SharedString,
    pub(crate) status_label: SharedString,
    pub(crate) total: SharedString,
}

/// Endereço já formatado para o card do detalhe.
#[derive(Clone)]
pub(crate) struct AddressRow {
    pub(crate) id: SharedString,
    pub(crate) label: SharedString,
    pub(crate) line: SharedString,
}

/// Dados de cliente com pixels já decodificados + métricas agregadas
/// dos pedidos. Thread-safe (Send) para passar pelo cache.
pub(crate) struct DecodedCustomer {
    pub(crate) id: SharedString,
    pub(crate) name: SharedString,
    pub(crate) email: SharedString,
    pub(crate) phone: SharedString,
    pub(crate) document: SharedString,
    pub(crate) avatar_initial: SharedString,
    pub(crate) notes: SharedString,
    pub(crate) created_at: SharedString,
    pub(crate) ltv: SharedString,
    pub(crate) ltv_pct: SharedString,
    pub(crate) order_count: i32,
    pub(crate) avg_ticket: SharedString,
    pub(crate) last_order: SharedString,
    pub(crate) last_order_rel: SharedString,
    pub(crate) status: SharedString,
    pub(crate) status_label: SharedString,
    pub(crate) is_vip: bool,
    pub(crate) recent: Vec<RecentOrder>,
    pub(crate) addresses: Vec<AddressRow>,
    pub(crate) pixel_buffer: Option<slint::SharedPixelBuffer<slint::Rgba8Pixel>>,
}

pub(crate) fn money(v: f64) -> String {
    // Formatação monetária única do sistema (separador de milhar + vírgula,
    // -0,0 normalizado). Delega ao helper canônico — a versão anterior usava
    // ponto decimal ("R$ 2530.00"), divergindo do resto (AI_RULES §8).
    crate::format::money_br_f64(v)
}

pub(crate) fn status_for(days: Option<i64>) -> (&'static str, &'static str) {
    match days {
        Some(d) if d <= 30 => ("ativo", "Ativo"),
        Some(d) if d <= 90 => ("atrasado", "Atrasado"),
        _ => ("inativo", "Inativo"),
    }
}

pub(crate) fn recency_label(days: Option<i64>) -> String {
    match days {
        None => "".to_string(),
        Some(d) if d <= 0 => "hoje".to_string(),
        Some(1) => "Há 1 Dia".to_string(),
        Some(d) => format!("Há {d} Dias"),
    }
}

/// Resumo dos itens: "2× Coca + 1× Pizza" (máx. 3 itens).
pub(crate) fn order_summary(o: &Order) -> String {
    let mut parts: Vec<String> = o.items.iter().take(3)
        .map(|i| format!("{}× {}", i.quantity as i64, i.product_name))
        .collect();
    if o.items.len() > 3 { parts.push("…".to_string()); }
    if parts.is_empty() { "—".to_string() } else { parts.join(" + ") }
}

pub(crate) fn status_label_pt(s: &OrderStatus) -> &'static str {
    match s {
        OrderStatus::Pending => "pendente",
        OrderStatus::Confirmed => "confirmado",
        OrderStatus::Preparing => "preparando",
        OrderStatus::Ready => "pronto",
        OrderStatus::Delivered => "entregue",
        OrderStatus::Cancelled => "cancelado",
    }
}

