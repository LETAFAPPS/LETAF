use std::time::Duration;

use leptos::prelude::*;

use crate::api::CatalogBanner;
use crate::format;

const ROTATE: Duration = Duration::from_millis(5000);

/// Carrossel de banners do topo do cardápio. O SSR mostra o 1º banner
/// (index=0) no HTML; após a hidratação ele auto-avança a cada 5s e os
/// bullets navegam. Estado puro de UI (§11). Banner `item_type="url"`
/// abre link externo; demais tipos ficam como imagem (o modal de
/// produto vem na slice do carrinho). Swipe touch fica para depois.
#[component]
pub fn BannerCarousel(banners: Vec<CatalogBanner>) -> impl IntoView {
    let total = banners.len();
    if total == 0 {
        return ().into_any();
    }
    let (index, set_index) = signal(0usize);

    // Auto-cycle só no cliente: o Effect não roda no SSR, então
    // `set_interval` (API do navegador) nunca é chamado no servidor.
    if total > 1 {
        Effect::new(move |_| {
            set_interval(move || set_index.update(|i| *i = (*i + 1) % total), ROTATE);
        });
    }

    view! {
        <div class="banner-carousel">
            <div
                class="banner-track"
                style=move || format!(
                    "transform:translateX(-{}00%);transition:transform .55s cubic-bezier(.22,.61,.36,1);",
                    index.get()
                )
            >
                {banners.into_iter().map(|b| {
                    let src = format::image_data_url(&b.image_data);
                    let title = b.title.clone();
                    let url = (b.item_type == "url").then_some(b.item_url).flatten();
                    match url {
                        Some(href) => view! {
                            <a class="banner-slide" href=href target="_blank" rel="noopener">
                                <img src=src alt=title draggable="false"/>
                            </a>
                        }.into_any(),
                        None => view! {
                            <div class="banner-slide">
                                <img src=src alt=title draggable="false"/>
                            </div>
                        }.into_any(),
                    }
                }).collect_view()}
            </div>
            {(total > 1).then(|| view! {
                <div class="banner-dots">
                    {(0..total).map(|i| view! {
                        <button
                            class="banner-dot"
                            class:banner-dot-active=move || index.get() == i
                            on:click=move |_| set_index.set(i)
                            aria-label="Ir para o banner"
                        ></button>
                    }).collect_view()}
                </div>
            })}
        </div>
    }
    .into_any()
}
