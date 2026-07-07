//! Lógica pura de seleção do modal (portada do web Dioxus). AI_RULES
//! §8/§11: é só ergonomia client-side; o backend revalida tudo
//! (`verify_item_prices`/`validate_variations`). Sem dependência de
//! Leptos — opera sobre os dados de seleção.

use std::collections::{HashMap, HashSet};

use crate::api::{CatalogAddonGroup, CatalogVariation};
use crate::cart::SelectedAddon;

// ───────────────────────── Adicionais ─────────────────────────

/// Lê o mapa `addon_id -> qty` de um grupo.
pub fn read_group_selection(
    state: &[(String, HashMap<String, u32>)],
    group_id: &str,
) -> HashMap<String, u32> {
    state
        .iter()
        .find(|(gid, _)| gid == group_id)
        .map(|(_, s)| s.clone())
        .unwrap_or_default()
}

/// Aplica um `delta` à quantidade de um addon no grupo. `single`: 0..1
/// (substitui); `multi`: counter livre respeitando `max_select` como
/// contagem de itens DISTINTOS.
pub fn change_addon_qty(
    state: &mut [(String, HashMap<String, u32>)],
    group_id: &str,
    addon_id: &str,
    delta: i32,
    is_single: bool,
    max_select: i32,
) {
    let Some(entry) = state.iter_mut().find(|(gid, _)| gid == group_id) else {
        return;
    };
    let current = entry.1.get(addon_id).copied().unwrap_or(0);

    if is_single {
        if delta < 0 {
            entry.1.remove(addon_id);
            return;
        }
        entry.1.clear();
        entry.1.insert(addon_id.to_string(), 1);
        return;
    }

    let next = (current as i64 + delta as i64).max(0) as u32;
    if next == 0 {
        entry.1.remove(addon_id);
        return;
    }
    if current == 0 && max_select > 0 {
        let distinct = entry.1.values().filter(|q| **q > 0).count() as i32;
        if distinct >= max_select {
            return;
        }
    }
    entry.1.insert(addon_id.to_string(), next);
}

/// Grupo válido: nº de itens distintos no range [min_select, max_select].
pub fn validate_group(g: &CatalogAddonGroup, selected: &HashMap<String, u32>) -> bool {
    let n = selected.values().filter(|q| **q > 0).count() as i32;
    let min = g.min_select.max(0);
    let max = g.max_select.max(0);
    n >= min && (max == 0 || n <= max)
}

/// Pode incrementar este addon? (multi sem teto, ou já selecionado, ou
/// ainda há espaço de itens distintos).
pub fn can_increment(group: &CatalogAddonGroup, qtys: &HashMap<String, u32>, current: u32) -> bool {
    let max = group.max_select.max(0) as u32;
    if max == 0 || current > 0 {
        return true;
    }
    let distinct = qtys.values().filter(|q| **q > 0).count() as u32;
    distinct < max
}

/// Badge curto ao lado do nome do grupo.
pub fn group_badge_label(g: &CatalogAddonGroup) -> String {
    if g.selection == "single" {
        if g.min_select >= 1 {
            "Obrigatório".into()
        } else {
            "Opcional".into()
        }
    } else if g.max_select > 0 {
        format!("até {} opções", g.max_select)
    } else if g.min_select >= 1 {
        format!("mínimo {}", g.min_select)
    } else {
        "Opcional".into()
    }
}

/// Snapshot final dos adicionais (preserva ordem; replica `qty` vezes).
pub fn build_snapshot(
    groups: &[CatalogAddonGroup],
    state: &[(String, HashMap<String, u32>)],
) -> Vec<SelectedAddon> {
    let mut out = Vec::new();
    for g in groups {
        let Some(sel) = state.iter().find(|(gid, _)| gid == &g.id).map(|(_, s)| s) else {
            continue;
        };
        for a in &g.addons {
            let qty = sel.get(&a.id).copied().unwrap_or(0);
            for _ in 0..qty {
                out.push(SelectedAddon {
                    name: a.name.clone(),
                    price: a.price,
                });
            }
        }
    }
    out
}

// ───────────────────────── Variações ──────────────────────────

/// Alterna a seleção de uma opção. `single` substitui; `multi`/
/// `max_value` alternam respeitando `max_select` (0 = sem limite).
pub fn toggle_variation_option(
    state: &mut [HashSet<usize>],
    var_idx: usize,
    opt_idx: usize,
    is_single: bool,
    max_select: i64,
) {
    let Some(entry) = state.get_mut(var_idx) else {
        return;
    };
    if is_single {
        entry.clear();
        entry.insert(opt_idx);
        return;
    }
    if entry.contains(&opt_idx) {
        entry.remove(&opt_idx);
    } else if max_select <= 0 || (entry.len() as i64) < max_select {
        entry.insert(opt_idx);
    }
}

/// Variação válida: `required` exige ≥1; min/max valem em multi/max_value.
pub fn validate_variation(v: &CatalogVariation, selected_count: usize) -> bool {
    let count = selected_count as i64;
    if v.required && count == 0 {
        return false;
    }
    if v.selection != "single" {
        let min = v.min_select.max(0);
        if min > 0 && count < min {
            return false;
        }
        if v.max_select > 0 && count > v.max_select {
            return false;
        }
    }
    true
}

/// Badge ao lado do título da variação.
pub fn variation_badge_label(v: &CatalogVariation) -> String {
    if v.required {
        "Obrigatório".into()
    } else {
        "Opcional".into()
    }
}

/// Dica do `max_value`: explica que só o maior preço entra no total.
pub fn max_value_hint(v: &CatalogVariation, selected: &HashSet<usize>) -> Option<String> {
    if v.selection != "max_value" {
        return None;
    }
    if selected.is_empty() {
        return Some("Você pode escolher várias — só o maior preço entra no total.".into());
    }
    let winner = selected
        .iter()
        .filter_map(|i| v.options.get(*i))
        .max_by(|a, b| a.price.partial_cmp(&b.price).unwrap_or(std::cmp::Ordering::Equal));
    winner.map(|w| format!("Cobrando a maior: {} (R$ {:.2}).", w.name, w.price))
}

/// Snapshot de variações. `max_value`: só a opção de maior preço entra.
pub fn build_variations_snapshot(
    variations: &[CatalogVariation],
    state: &[HashSet<usize>],
) -> Vec<SelectedAddon> {
    let mut out = Vec::new();
    for (idx, v) in variations.iter().enumerate() {
        let Some(sel) = state.get(idx) else { continue };
        if sel.is_empty() {
            continue;
        }
        if v.selection == "max_value" {
            let winner = sel
                .iter()
                .filter_map(|i| v.options.get(*i))
                .max_by(|a, b| a.price.partial_cmp(&b.price).unwrap_or(std::cmp::Ordering::Equal));
            if let Some(o) = winner {
                out.push(SelectedAddon {
                    name: o.name.clone(),
                    price: o.price,
                });
            }
            continue;
        }
        for (oidx, opt) in v.options.iter().enumerate() {
            if sel.contains(&oidx) {
                out.push(SelectedAddon {
                    name: opt.name.clone(),
                    price: opt.price,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::*;
    use crate::api::{CatalogAddon, CatalogAddonGroup, CatalogVariation, CatalogVariationOption};

    fn variation(selection: &str, opts: &[(&str, f64)]) -> CatalogVariation {
        CatalogVariation {
            title: "V".into(),
            selection: selection.into(),
            required: false,
            min_select: 0,
            max_select: 0,
            options: opts
                .iter()
                .map(|(n, p)| CatalogVariationOption {
                    name: (*n).into(),
                    price: *p,
                })
                .collect(),
        }
    }

    fn group(selection: &str, min: i32, max: i32, addons: &[(&str, &str, f64)]) -> CatalogAddonGroup {
        CatalogAddonGroup {
            id: "g".into(),
            name: "G".into(),
            selection: selection.into(),
            min_select: min,
            max_select: max,
            addons: addons
                .iter()
                .map(|(id, n, p)| CatalogAddon {
                    id: (*id).into(),
                    name: (*n).into(),
                    price: *p,
                })
                .collect(),
        }
    }

    #[test]
    fn variation_single_replaces() {
        let mut st = vec![HashSet::new()];
        toggle_variation_option(&mut st, 0, 0, true, 0);
        toggle_variation_option(&mut st, 0, 1, true, 0);
        assert_eq!(st[0].len(), 1);
        assert!(st[0].contains(&1));
    }

    #[test]
    fn variation_max_value_charges_highest() {
        let v = variation("max_value", &[("A", 3.0), ("B", 7.0), ("C", 5.0)]);
        let mut st = vec![HashSet::new()];
        for i in 0..3 {
            toggle_variation_option(&mut st, 0, i, false, 0);
        }
        let snap = build_variations_snapshot(std::slice::from_ref(&v), &st);
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].name, "B");
        assert_eq!(snap[0].price, 7.0);
    }

    #[test]
    fn variation_multi_caps_at_max_select() {
        let mut st = vec![HashSet::new()];
        toggle_variation_option(&mut st, 0, 0, false, 2);
        toggle_variation_option(&mut st, 0, 1, false, 2);
        toggle_variation_option(&mut st, 0, 2, false, 2); // bloqueado
        assert_eq!(st[0].len(), 2);
    }

    #[test]
    fn variation_required_validation() {
        let mut v = variation("single", &[("P", 0.0)]);
        v.required = true;
        assert!(!validate_variation(&v, 0));
        assert!(validate_variation(&v, 1));
    }

    #[test]
    fn addon_snapshot_replicates_qty() {
        let g = group("multi", 0, 0, &[("a1", "Bacon", 2.0)]);
        let mut st = vec![("g".to_string(), HashMap::new())];
        change_addon_qty(&mut st, "g", "a1", 1, false, 0);
        change_addon_qty(&mut st, "g", "a1", 1, false, 0);
        let snap = build_snapshot(std::slice::from_ref(&g), &st);
        assert_eq!(snap.len(), 2);
        assert_eq!(snap[0].name, "Bacon");
    }

    #[test]
    fn addon_single_swaps_and_validates_min() {
        let g = group("single", 1, 0, &[("a1", "P", 0.0), ("a2", "G", 3.0)]);
        let mut st = vec![("g".to_string(), HashMap::new())];
        change_addon_qty(&mut st, "g", "a1", 1, true, 0);
        change_addon_qty(&mut st, "g", "a2", 1, true, 0); // troca
        let sel = read_group_selection(&st, "g");
        assert_eq!(sel.values().filter(|q| **q > 0).count(), 1);
        assert!(validate_group(&g, &sel));
    }

    #[test]
    fn addon_multi_caps_distinct_at_max() {
        let g = group("multi", 0, 1, &[("a1", "X", 1.0), ("a2", "Y", 1.0)]);
        let mut st = vec![("g".to_string(), HashMap::new())];
        change_addon_qty(&mut st, "g", "a1", 1, false, 1);
        change_addon_qty(&mut st, "g", "a2", 1, false, 1); // bloqueado
        let sel = read_group_selection(&st, "g");
        assert_eq!(sel.values().filter(|q| **q > 0).count(), 1);
        assert!(validate_group(&g, &sel)); // 1 item selecionado respeita max=1
    }
}
