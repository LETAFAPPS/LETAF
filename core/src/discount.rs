//! Cálculo de desconto aplicado ao preço unitário de um produto.
//!
//! Regras aplicadas (AI_RULES.md §1, §11, §13):
//! - Função pura, sem dependência de banco/UI — roda no backend (validar
//!   `unit_price` do cliente) e no cliente web (mostrar). Mesma lógica.
//! - Desconto incide só sobre o preço base; adicionais somam depois.
//! - Dinheiro em `Decimal` (exato); quantidades seguem `f64`.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::product::model::Product;

/// Preço unitário do produto com o desconto eventual aplicado, para a
/// `quantity` informada (descontos `bulk_*` dependem da quantidade).
pub fn effective_unit_price(p: &Product, quantity: f64) -> Decimal {
    let base = p.price.unwrap_or(Decimal::ZERO);
    let Some(kind) = p.discount_kind.as_deref() else { return base; };
    match kind {
        "fixed" => (base - p.discount_value.unwrap_or(Decimal::ZERO)).max(Decimal::ZERO),
        "percent" => {
            let pct = p.discount_value.unwrap_or(Decimal::ZERO);
            (base * (dec!(1) - pct / dec!(100))).max(Decimal::ZERO)
        }
        "bulk_fixed" => match winning_bulk_value(p, quantity) {
            Some(v) => (base - v).max(Decimal::ZERO),
            None => base,
        },
        "bulk_percent" => match winning_bulk_value(p, quantity) {
            Some(v) => (base * (dec!(1) - v / dec!(100))).max(Decimal::ZERO),
            None => base,
        },
        _ => base,
    }
}

/// Tier vencedor (maior `min_qty` satisfeito) — devolve o `value`.
fn winning_bulk_value(p: &Product, quantity: f64) -> Option<Decimal> {
    if let Some(json) = p.discount_tiers.as_deref() {
        if let Some(value) = winning_tier_from_json(json, quantity) {
            return Some(value);
        }
    }
    let min = p.discount_min_qty?;
    let v = p.discount_value?;
    if min > 0.0 && quantity >= min { Some(v) } else { None }
}

fn winning_tier_from_json(json: &str, quantity: f64) -> Option<Decimal> {
    let parsed: serde_json::Value = serde_json::from_str(json).ok()?;
    let arr = parsed.as_array()?;
    // `min_qty` é quantidade (f64); `value` é dinheiro (Decimal).
    let mut tiers: Vec<(f64, Decimal)> = arr.iter()
        .filter_map(|v| {
            let q = v.get("min_qty")?.as_f64()?;
            let val = crate::money::price_from_json(v.get("value")?)?;
            if q <= 0.0 { return None; }
            Some((q, val))
        })
        .collect();
    if tiers.is_empty() { return None; }
    tiers.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    tiers.into_iter().rev().find(|(q, _)| quantity >= *q).map(|(_, v)| v)
}
