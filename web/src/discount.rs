//! Cálculo do desconto de EXIBIÇÃO no card (função pura, portada do web
//! Dioxus). AI_RULES.md §1/§8/§11: é só apresentação — a autoridade do
//! preço fica no backend (`order::service::verify_item_prices`). Mesma
//! lógica do core, sem duplicar regra de negócio na UI.

use crate::api::CatalogProduct;

#[derive(Clone, Copy, Debug)]
struct BulkTier {
    min_qty: f64,
    value: f64,
}

fn parse_tiers(json: &str) -> Vec<BulkTier> {
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let Some(arr) = parsed.as_array() else {
        return Vec::new();
    };
    let mut tiers: Vec<BulkTier> = arr
        .iter()
        .filter_map(|v| {
            let obj = v.as_object()?;
            let min_qty = obj.get("min_qty").and_then(|x| x.as_f64())?;
            // Tolerante: valor número (legado) ou string decimal (novo formato).
            let value = obj
                .get("value")
                .and_then(|x| x.as_f64().or_else(|| x.as_str().and_then(|s| s.trim().parse().ok())))?;
            if min_qty <= 0.0 {
                return None;
            }
            Some(BulkTier { min_qty, value })
        })
        .collect();
    tiers.sort_by(|a, b| a.min_qty.partial_cmp(&b.min_qty).unwrap_or(std::cmp::Ordering::Equal));
    tiers
}

fn winning_tier(tiers: &[BulkTier], qty: f64) -> Option<BulkTier> {
    tiers.iter().rev().copied().find(|t| qty >= t.min_qty)
}

fn resolve_bulk_tier(p: &CatalogProduct, qty: f64) -> Option<BulkTier> {
    if let Some(json) = p.discount_tiers.as_deref() {
        let tiers = parse_tiers(json);
        if !tiers.is_empty() {
            return winning_tier(&tiers, qty);
        }
    }
    let min_qty = p.discount_min_qty?;
    let value = p.discount_value?;
    if min_qty > 0.0 && qty >= min_qty {
        Some(BulkTier { min_qty, value })
    } else {
        None
    }
}

fn lowest_bulk_tier(p: &CatalogProduct) -> Option<BulkTier> {
    if let Some(json) = p.discount_tiers.as_deref() {
        let tiers = parse_tiers(json);
        if let Some(first) = tiers.first() {
            return Some(*first);
        }
    }
    let min_qty = p.discount_min_qty?;
    let value = p.discount_value?;
    if min_qty > 0.0 {
        Some(BulkTier { min_qty, value })
    } else {
        None
    }
}

/// Preço unitário com o desconto aplicável (qty=1 no card).
pub fn effective_unit_price(p: &CatalogProduct, qty: f64) -> f64 {
    let base = p.price.unwrap_or(0.0);
    let Some(kind) = p.discount_kind.as_deref() else {
        return base;
    };
    match kind {
        "fixed" => (base - p.discount_value.unwrap_or(0.0)).max(0.0),
        "percent" => (base * (1.0 - p.discount_value.unwrap_or(0.0) / 100.0)).max(0.0),
        "bulk_fixed" => resolve_bulk_tier(p, qty)
            .map(|t| (base - t.value).max(0.0))
            .unwrap_or(base),
        "bulk_percent" => resolve_bulk_tier(p, qty)
            .map(|t| (base * (1.0 - t.value / 100.0)).max(0.0))
            .unwrap_or(base),
        _ => base,
    }
}

/// Rótulo do selo de desconto. `bulk_*` anuncia o menor gatilho.
pub fn discount_badge_label(p: &CatalogProduct) -> Option<String> {
    let kind = p.discount_kind.as_deref()?;
    match kind {
        "fixed" => Some(format!("R$ {:.2} Off", p.discount_value.unwrap_or(0.0))),
        "percent" => Some(format!("{:.0}% Off", p.discount_value.unwrap_or(0.0))),
        "bulk_fixed" => {
            let t = lowest_bulk_tier(p)?;
            Some(format!("Acima de {:.0} un.: R$ {:.2} Off", t.min_qty, t.value))
        }
        "bulk_percent" => {
            let t = lowest_bulk_tier(p)?;
            Some(format!("Acima de {:.0} un.: {:.0}% Off", t.min_qty, t.value))
        }
        _ => None,
    }
}

/// `true` se há desconto unitário ativo (mostra o preço base riscado).
pub fn has_active_unit_discount(p: &CatalogProduct, qty: f64) -> bool {
    let base = p.price.unwrap_or(0.0);
    base > 0.0 && (effective_unit_price(p, qty) - base).abs() > 0.001
}
