use leptos::prelude::*;

use crate::api::CatalogProduct;
use crate::availability::Now;
use crate::cart::Cart;
use crate::components::product_modal::ProductModal;
use crate::favorites::{self, Favorites};
use crate::{availability, discount, format};

/// Card de produto do cardápio: imagem (com cor de fundo do upload),
/// botão de favorito, selo de desconto, nome, descrição e preço (base
/// riscado + com desconto quando houver).
#[component]
pub fn ProductCard(product: CatalogProduct) -> impl IntoView {
    let price = product.price.unwrap_or(0.0);
    let unit = discount::effective_unit_price(&product, 1.0);
    let has_disc = discount::has_active_unit_discount(&product, 1.0);
    let seal = discount::discount_badge_label(&product);

    let img = product.image_data.as_deref().map(format::image_data_url);
    let bg = product
        .cover_color
        .clone()
        .filter(|c| !c.is_empty())
        .unwrap_or_else(|| "#ffffff".to_string());
    let name = product.name.clone();
    let alt = product.name.clone();
    let desc = product.description.clone().unwrap_or_default();

    // Favorito: preferência de UI compartilhada por contexto (§11 — sem
    // lógica de negócio no cliente). No SSR o conjunto é vazio → coração
    // neutro; após a hidratação reflete o localStorage.
    let favs = expect_context::<Favorites>();
    let id_read = product.id.clone();
    let id_toggle = product.id.clone();
    let is_fav = move || favs.0.with(|s| s.contains(&id_read));
    let toggle = move |_| {
        favs.0.update(|s| {
            if !s.remove(&id_toggle) {
                s.insert(id_toggle.clone());
            }
        });
        favorites::save(&favs.0.get_untracked());
    };

    // Carrinho: produtos com adicionais/variações abrem o modal (escolha
    // + qty); produtos simples vão direto (qty 1) — UX de delivery.
    let cart = expect_context::<Cart>();
    let has_options = !product.addon_groups.is_empty() || !product.variations.is_empty();
    let product_sv = StoredValue::new(product.clone());
    let (modal_open, set_modal_open) = signal(false);

    // Disponibilidade por horário (§3): no SSR `now=None` → disponível;
    // após a hidratação reflete o relógio do cliente. Reativo via `Now`.
    let now = expect_context::<Now>();
    let sched = product.availability_schedule.clone();
    let available = Memo::new(move |_| availability::is_available_now(sched.as_deref(), now.0.get()));
    let on_add = move |_| {
        if has_options {
            set_modal_open.set(true);
        } else {
            product_sv.with_value(|p| cart.add(p.clone(), 1.0, Vec::new()));
        }
    };

    view! {
        <article class="product-card">
            <div class="product-img" style=format!("background:{bg};")>
                {match img {
                    Some(src) => view! { <img src=src alt=alt loading="lazy"/> }.into_any(),
                    None => view! { <span class="no-image">"sem imagem"</span> }.into_any(),
                }}
                {seal.map(|s| view! { <span class="discount-seal">{s}</span> })}
                <button class="fav" class:fav-on=is_fav on:click=toggle aria-label="Favoritar">
                    "♥"
                </button>
                {move || (!available.get()).then(|| view! {
                    <div class="unavailable"><span>"Indisponível"</span></div>
                })}
            </div>
            <div class="product-body">
                <h3 class="product-name">{name}</h3>
                {(!desc.is_empty()).then(|| view! { <p class="product-desc">{desc}</p> })}
                <div class="price">
                    {if has_disc {
                        view! {
                            <span class="price-old">{format::money(price)}</span>
                            <span class="price-now">{format::money(unit)}</span>
                        }.into_any()
                    } else {
                        view! { <span class="price-now">{format::money(price)}</span> }.into_any()
                    }}
                </div>
                <button class="add-btn" on:click=on_add disabled=move || !available.get()>
                    {move || if available.get() { "+ Adicionar" } else { "Indisponível" }}
                </button>
            </div>
            {move || modal_open.get().then(|| view! {
                <ProductModal
                    product=product_sv.get_value()
                    on_close=Callback::new(move |_| set_modal_open.set(false))
                />
            })}
        </article>
    }
}
