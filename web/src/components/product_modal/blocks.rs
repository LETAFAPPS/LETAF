//! Render dos blocos do modal (variações e grupos de adicionais). Cada
//! linha recebe o estado atual (snapshot) e muta o `RwSignal` no clique;
//! o corpo do modal re-renderiza ao mudar a seleção.

use std::collections::{HashMap, HashSet};

use leptos::prelude::*;

use crate::api::{CatalogAddonGroup, CatalogVariation};
use crate::format;

use super::logic;

/// Bloco de uma variação: título + badge + dica (`max_value`) + opções.
pub fn render_variation(
    idx: usize,
    v: CatalogVariation,
    selected: HashSet<usize>,
    var_selection: RwSignal<Vec<HashSet<usize>>>,
) -> AnyView {
    let is_single = v.selection == "single";
    let max_sel = v.max_select;
    let badge = logic::variation_badge_label(&v);
    let hint = logic::max_value_hint(&v, &selected);
    let title = v.title.clone();

    let rows = v
        .options
        .iter()
        .enumerate()
        .map(|(oidx, opt)| {
            let is_sel = selected.contains(&oidx);
            let name = opt.name.clone();
            let price_txt = price_label(opt.price);
            let ind = indicator_class(is_single, is_sel);
            let mark = if !is_single && is_sel { "✓" } else { "" };
            view! {
                <button
                    class="opt-row"
                    class:opt-sel=is_sel
                    on:click=move |_| var_selection.update(|s| {
                        logic::toggle_variation_option(s, idx, oidx, is_single, max_sel);
                    })
                >
                    <span class=ind>{mark}</span>
                    <span class="opt-name">{name}</span>
                    <span class="opt-price">{price_txt}</span>
                </button>
            }
        })
        .collect_view();

    view! {
        <div class="pm-block">
            <div class="pm-block-head">
                <span class="pm-block-title">{title}</span>
                <span class="pm-badge">{badge}</span>
            </div>
            {hint.map(|h| view! { <div class="pm-hint">{h}</div> })}
            <div class="pm-options">{rows}</div>
        </div>
    }
    .into_any()
}

/// Bloco de um grupo de adicionais (radio em `single`, checkbox/counter
/// em `multi`).
pub fn render_group(
    g: CatalogAddonGroup,
    qtys: HashMap<String, u32>,
    selection: RwSignal<Vec<(String, HashMap<String, u32>)>>,
) -> AnyView {
    let is_single = g.selection == "single";
    let max_sel = g.max_select;
    let badge = logic::group_badge_label(&g);
    let title = g.name.clone();
    let gid = g.id.clone();

    let rows = g
        .addons
        .iter()
        .map(|addon| {
            let qty = qtys.get(&addon.id).copied().unwrap_or(0);
            let selected = qty > 0;
            let can_inc = logic::can_increment(&g, &qtys, qty);
            let name = addon.name.clone();
            let price_txt = price_label(addon.price);
            let aid = addon.id.clone();

            if is_single {
                let (g1, a1) = (gid.clone(), aid.clone());
                view! {
                    <button
                        class="opt-row"
                        class:opt-sel=selected
                        on:click=move |_| {
                            let delta = if selected { -1 } else { 1 };
                            selection.update(|s| {
                                logic::change_addon_qty(s, &g1, &a1, delta, true, max_sel);
                            });
                        }
                    >
                        <span class=indicator_class(true, selected)></span>
                        <span class="opt-name">{name}</span>
                        <span class="opt-price">{price_txt}</span>
                    </button>
                }
                .into_any()
            } else if qty == 0 {
                let (g1, a1) = (gid.clone(), aid.clone());
                view! {
                    <button
                        class="opt-row"
                        disabled=!can_inc
                        on:click=move |_| {
                            if can_inc {
                                selection.update(|s| {
                                    logic::change_addon_qty(s, &g1, &a1, 1, false, max_sel);
                                });
                            }
                        }
                    >
                        <span class=indicator_class(false, false)></span>
                        <span class="opt-name">{name}</span>
                        <span class="opt-price">{price_txt}</span>
                    </button>
                }
                .into_any()
            } else {
                let (gc, ac) = (gid.clone(), aid.clone());
                let (gd, ad) = (gid.clone(), aid.clone());
                let (gi, ai) = (gid.clone(), aid.clone());
                let qty_i = qty as i32;
                view! {
                    <div class="opt-row opt-sel">
                        <button
                            class="ind check on"
                            title="Remover"
                            on:click=move |_| selection.update(|s| {
                                logic::change_addon_qty(s, &gc, &ac, -qty_i, false, max_sel);
                            })
                        >
                            "✓"
                        </button>
                        <span class="opt-name">{name}</span>
                        <span class="opt-price">{price_txt}</span>
                        <div class="opt-counter">
                            <button on:click=move |_| selection.update(|s| {
                                logic::change_addon_qty(s, &gd, &ad, -1, false, max_sel);
                            })>"−"</button>
                            <span>{qty.to_string()}</span>
                            <button
                                disabled=!can_inc
                                on:click=move |_| {
                                    if can_inc {
                                        selection.update(|s| {
                                            logic::change_addon_qty(s, &gi, &ai, 1, false, max_sel);
                                        });
                                    }
                                }
                            >
                                "+"
                            </button>
                        </div>
                    </div>
                }
                .into_any()
            }
        })
        .collect_view();

    view! {
        <div class="pm-block">
            <div class="pm-block-head">
                <span class="pm-block-title">{title}</span>
                <span class="pm-badge">{badge}</span>
            </div>
            <div class="pm-options">{rows}</div>
        </div>
    }
    .into_any()
}

fn price_label(price: f64) -> String {
    if price > 0.0 {
        format!("+ {}", format::money(price))
    } else {
        "Grátis".to_string()
    }
}

fn indicator_class(is_radio: bool, on: bool) -> String {
    let shape = if is_radio { "radio" } else { "check" };
    if on {
        format!("ind {shape} on")
    } else {
        format!("ind {shape}")
    }
}
