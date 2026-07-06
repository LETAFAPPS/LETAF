//! Cálculo de desconto aplicado ao preço unitário de um produto.
//!
//! Regras aplicadas (AI_RULES.md §1, §11):
//! - Função pura, sem dependência de banco/UI — pode ser chamada tanto
//!   pelo backend (validar `unit_price` do cliente) quanto pelo cliente
//!   (calcular o que mostrar). Mesma lógica → mesmo resultado.
//! - Desconto incide só sobre o preço base do produto; adicionais
//!   (Fase 4) são acréscimo limpo somado depois.

use crate::product::model::Product;

/// Preço unitário do produto com o desconto eventual aplicado, para a
/// `quantity` informada (descontos `bulk_*` dependem da quantidade).
///
/// Cobre os 4 kinds suportados (`fixed`, `percent`, `bulk_fixed`,
/// `bulk_percent`) e ambos os modos bulk: tier único legado em
/// `discount_value`/`discount_min_qty` OU múltiplos tiers em
/// `discount_tiers` (JSON `[{"min_qty","value"}, ...]`).
pub fn effective_unit_price(p: &Product, quantity: f64) -> f64 {
    let base = p.price.unwrap_or(0.0);
    let Some(kind) = p.discount_kind.as_deref() else { return base; };
    match kind {
        "fixed" => (base - p.discount_value.unwrap_or(0.0)).max(0.0),
        "percent" => (base * (1.0 - p.discount_value.unwrap_or(0.0) / 100.0)).max(0.0),
        "bulk_fixed" => match winning_bulk_value(p, quantity) {
            Some(v) => (base - v).max(0.0),
            None => base,
        },
        "bulk_percent" => match winning_bulk_value(p, quantity) {
            Some(v) => (base * (1.0 - v / 100.0)).max(0.0),
            None => base,
        },
        _ => base,
    }
}

/// Tier vencedor (maior `min_qty` cuja condição `quantity >= min_qty` é
/// satisfeita) — devolve apenas o `value` para uso na fórmula.
/// Considera os dois formatos suportados (tiers JSON ou tier único).
fn winning_bulk_value(p: &Product, quantity: f64) -> Option<f64> {
    if let Some(json) = p.discount_tiers.as_deref() {
        if let Some(value) = winning_tier_from_json(json, quantity) {
            return Some(value);
        }
    }
    let min = p.discount_min_qty?;
    let v = p.discount_value?;
    if min > 0.0 && quantity >= min { Some(v) } else { None }
}

fn winning_tier_from_json(json: &str, quantity: f64) -> Option<f64> {
    let parsed: serde_json::Value = serde_json::from_str(json).ok()?;
    let arr = parsed.as_array()?;
    let mut tiers: Vec<(f64, f64)> = arr.iter()
        .filter_map(|v| {
            let q = v.get("min_qty")?.as_f64()?;
            let val = v.get("value")?.as_f64()?;
            if q <= 0.0 { return None; }
            Some((q, val))
        })
        .collect();
    if tiers.is_empty() { return None; }
    tiers.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    tiers.into_iter().rev().find(|(q, _)| quantity >= *q).map(|(_, v)| v)
}
