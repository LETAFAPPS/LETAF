use leptos::prelude::*;

use crate::api::CatalogProduct;
use crate::availability::Now;
use crate::cart::Cart;
use crate::components::icon::Icon;
use crate::components::catalog::StoreOpen;
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

    // Galeria (loja): usa image_urls quando houver; senão cai na principal.
    let images: Vec<String> = if product.image_urls.is_empty() {
        product.image_url.clone().map(|u| vec![u]).unwrap_or_default()
    } else {
        product.image_urls.clone()
    };
    let n_imgs = images.len();
    let images_sv = StoredValue::new(images);
    let (img_idx, set_img_idx) = signal(0usize);
    // Cor de fundo do upload (quando houver). Sem cor, o `.product-img`
    // usa o token de superfície do CSS — nada de bloco branco fixo, que
    // destoava no modo escuro.
    let bg_style = product
        .cover_color
        .clone()
        .filter(|c| !c.is_empty())
        .map(|c| format!("background:{c};"))
        .unwrap_or_default();
    let name = product.name.clone();
    let alt = product.name.clone();

    // Favorito: preferência de UI compartilhada por contexto (§11 — sem
    // lógica de negócio no cliente). No SSR o conjunto é vazio → coração
    // neutro; após a hidratação reflete o localStorage.
    let favs = expect_context::<Favorites>();
    let id_read = product.id.clone();
    let id_toggle = product.id.clone();
    // Memo (Copy) para reusar em `class:` e nos aria-* do botão sem
    // reclonar o id — reativo ao conjunto de favoritos.
    let is_fav = Memo::new(move |_| favs.0.with(|s| s.contains(&id_read)));
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

    // Disponibilidade (§3, só exibição): indisponível quando a LOJA está
    // FECHADA ou o produto está fora do horário dele. No SSR `now=None` e
    // `store_open=true` → disponível (bom p/ SEO, sem mismatch); após a
    // hidratação reflete o relógio/horários. O backend recusa o pedido de
    // qualquer forma (§11).
    let now = expect_context::<Now>();
    let store_open = expect_context::<StoreOpen>();
    let sched = product.availability_schedule.clone();
    let available = Memo::new(move |_| {
        store_open.0.get() && availability::is_available_now(sched.as_deref(), now.0.get())
    });
    let on_add = move |_| {
        if has_options {
            set_modal_open.set(true);
        } else {
            product_sv.with_value(|p| cart.add(p.clone(), 1.0, Vec::new()));
        }
    };

    view! {
        <article class="product-card" class:card-unavailable=move || !available.get()>
            <div class="product-img" style=bg_style>
                {if n_imgs == 0 {
                    view! { <span class="no-image">"sem imagem"</span> }.into_any()
                } else if n_imgs == 1 {
                    let src = images_sv.with_value(|v| v[0].clone());
                    view! { <img src=src alt=alt loading="lazy"/> }.into_any()
                } else {
                    // Carrossel: imagem corrente + navegação + bolinhas.
                    view! {
                        <img
                            src=move || images_sv.with_value(|v| v[img_idx.get().min(v.len() - 1)].clone())
                            alt=alt loading="lazy"/>
                        <button class="carousel-nav prev" aria-label="Anterior"
                            on:click=move |_| set_img_idx.update(|i| *i = (*i + n_imgs - 1) % n_imgs)>
                            <Icon name="seta-esquerda"/>
                        </button>
                        <button class="carousel-nav next" aria-label="Próxima"
                            on:click=move |_| set_img_idx.update(|i| *i = (*i + 1) % n_imgs)>
                            <Icon name="seta-direita"/>
                        </button>
                        <div class="carousel-dots">
                            {(0..n_imgs).map(|k| view! {
                                <button class="dot" class:dot-on=move || img_idx.get() == k
                                    aria-label=move || format!("Ver imagem {}", k + 1)
                                    on:click=move |_| set_img_idx.set(k)></button>
                            }).collect_view()}
                        </div>
                    }.into_any()
                }}
                {seal.map(|s| view! { <span class="discount-seal">{s}</span> })}
                <button class="fav" class:fav-on=move || is_fav.get() on:click=toggle
                    aria-pressed=move || is_fav.get().to_string()
                    aria-label=move || if is_fav.get() { "Remover dos favoritos" } else { "Adicionar aos favoritos" }>
                    <Icon name="favoritos"/>
                </button>
                {move || (!available.get()).then(|| view! {
                    <div class="unavailable"><span>"Indisponível"</span></div>
                })}
            </div>
            <div class="product-body">
                <h3 class="product-name">{name}</h3>
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
