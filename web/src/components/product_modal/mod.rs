use std::collections::{HashMap, HashSet};

use leptos::prelude::*;

use crate::api::CatalogProduct;
use crate::cart::Cart;
use crate::{discount, format};

mod blocks;
mod logic;

/// Modal de produto: descrição, desconto, variações, adicionais e
/// quantidade. A validação aqui é só ergonomia (§11) — o backend
/// revalida tudo no checkout. Ao confirmar, monta o snapshot
/// (variações primeiro, depois adicionais) e adiciona ao carrinho.
#[component]
pub fn ProductModal(product: CatalogProduct, on_close: Callback<()>) -> impl IntoView {
    let cart = expect_context::<Cart>();

    let name = product.name.clone();
    let description = product.description.clone().unwrap_or_default();
    let seal = discount::discount_badge_label(&product);
    let raw_base = product.price.unwrap_or(0.0);

    let groups_init: Vec<(String, HashMap<String, u32>)> = product
        .addon_groups
        .iter()
        .map(|g| (g.id.clone(), HashMap::new()))
        .collect();
    let vars_init: Vec<HashSet<usize>> =
        product.variations.iter().map(|_| HashSet::new()).collect();

    let selection = RwSignal::new(groups_init);
    let var_selection = RwSignal::new(vars_init);
    let qty = RwSignal::new(1u32);

    let product = StoredValue::new(product);

    // Preço base (reativo na qty: desconto pode variar por quantidade).
    let base_price =
        move || product.with_value(|p| discount::effective_unit_price(p, qty.get() as f64));
    let has_disc =
        move || product.with_value(|p| discount::has_active_unit_discount(p, qty.get() as f64));

    // Total = (base + extras) * qty.
    let total = move || {
        let q = qty.get() as f64;
        product.with_value(|p| {
            let base = discount::effective_unit_price(p, q);
            let extras: f64 = logic::build_snapshot(&p.addon_groups, &selection.get())
                .iter()
                .chain(logic::build_variations_snapshot(&p.variations, &var_selection.get()).iter())
                .map(|s| s.price)
                .sum();
            (base + extras) * q
        })
    };

    // Validade: todos os grupos e variações dentro das regras.
    let all_valid = move || {
        product.with_value(|p| {
            let groups_ok = p.addon_groups.iter().all(|g| {
                let sel = logic::read_group_selection(&selection.get(), &g.id);
                logic::validate_group(g, &sel)
            });
            let vars_ok = p.variations.iter().enumerate().all(|(idx, v)| {
                let count = var_selection.with(|s| s.get(idx).map(|x| x.len()).unwrap_or(0));
                logic::validate_variation(v, count)
            });
            groups_ok && vars_ok
        })
    };

    let confirm = move |_| {
        if !all_valid() {
            return;
        }
        let q = qty.get() as f64;
        let snap = product.with_value(|p| {
            let mut s = logic::build_variations_snapshot(&p.variations, &var_selection.get());
            s.extend(logic::build_snapshot(&p.addon_groups, &selection.get()));
            s
        });
        product.with_value(|p| cart.add(p.clone(), q, snap.clone()));
        on_close.run(());
    };

    view! {
        <div class="modal-overlay" on:click=move |_| on_close.run(())>
            <div class="product-modal" on:click=|e: leptos::ev::MouseEvent| e.stop_propagation()>
                <header class="pm-head">
                    <div class="pm-head-text">
                        <div class="pm-name">{name}</div>
                        {(!description.is_empty())
                            .then(|| view! { <div class="pm-desc">{description}</div> })}
                        <div class="pm-price">
                            {move || if has_disc() {
                                view! {
                                    <span class="price-old">{format::money(raw_base)}</span>
                                    <span class="price-now">{format::money(base_price())}</span>
                                }.into_any()
                            } else {
                                view! {
                                    <span class="price-now">{format::money(base_price())}</span>
                                }.into_any()
                            }}
                        </div>
                    </div>
                    {seal.map(|s| view! { <span class="discount-seal pm-seal">{s}</span> })}
                    <button class="cart-close" on:click=move |_| on_close.run(()) aria-label="Fechar">
                        "✕"
                    </button>
                </header>

                <div class="pm-body">
                    {move || {
                        let sel = selection.get();
                        let vsel = var_selection.get();
                        product.with_value(|p| {
                            let mut out = Vec::new();
                            for (idx, v) in p.variations.iter().enumerate() {
                                let selected = vsel.get(idx).cloned().unwrap_or_default();
                                out.push(blocks::render_variation(
                                    idx, v.clone(), selected, var_selection,
                                ));
                            }
                            for g in &p.addon_groups {
                                let qtys = sel
                                    .iter()
                                    .find(|(gid, _)| gid == &g.id)
                                    .map(|(_, s)| s.clone())
                                    .unwrap_or_default();
                                out.push(blocks::render_group(g.clone(), qtys, selection));
                            }
                            out.into_iter().collect_view()
                        })
                    }}
                </div>

                <footer class="pm-foot">
                    <div class="cart-qty">
                        <button on:click=move |_| qty.update(|q| { if *q > 1 { *q -= 1; } })>"−"</button>
                        <span>{move || qty.get().to_string()}</span>
                        <button on:click=move |_| qty.update(|q| *q += 1)>"+"</button>
                    </div>
                    <div class="pm-total">
                        <div class="pm-total-label">"Total"</div>
                        <div class="pm-total-value">{move || format::money(total())}</div>
                    </div>
                    <button class="pm-add" disabled=move || !all_valid() on:click=confirm>
                        "Adicionar"
                    </button>
                </footer>
            </div>
        </div>
    }
}
