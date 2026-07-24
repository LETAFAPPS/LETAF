use leptos::prelude::*;
use leptos_meta::{Meta, Title};

use crate::api::CatalogData;
use crate::availability::{self, Now};
use super::account_button::AccountButton;
use super::banner_carousel::BannerCarousel;
use super::product_card::ProductCard;

/// Server function: lê o `Host` da requisição SSR, resolve o tenant e
/// busca o catálogo público na API (server-side). No cliente vira uma
/// chamada HTTP a este servidor SSR — o navegador nunca fala direto com
/// a API para o catálogo (AI_RULES §1/§11, frontend burro).
#[server]
pub async fn get_catalog() -> Result<CatalogData, ServerFnError> {
    use axum::http::{header::HOST, HeaderMap};
    let headers: HeaderMap = leptos_axum::extract().await?;
    let host = headers
        .get(HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    // Esquema público: atrás de proxy vale o `x-forwarded-proto`; senão
    // http para localhost (dev) e https no resto (produção).
    let proto = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .unwrap_or_else(|| {
            if host.starts_with("localhost") || host.contains(".localhost") || host.starts_with("127.0.0.1") {
                "http".into()
            } else {
                "https".into()
            }
        });
    let mut data = crate::api::fetch_catalog(&host)
        .await
        .map_err(ServerFnError::new)?;
    data.site_origin = format!("{proto}://{host}");
    Ok(data)
}

/// Página do cardápio (home). Resource bloqueante → o HTML inicial já
/// sai completo (SEO).
#[component]
pub fn CatalogPage() -> impl IntoView {
    let catalog = Resource::new_blocking(|| (), |_| get_catalog());

    view! {
        <Suspense fallback=|| view! { <p class="state">"Carregando cardápio…"</p> }>
            {move || Suspend::new(async move {
                match catalog.await {
                    Ok(data) => view! { <CatalogView data/> }.into_any(),
                    Err(e) => view! {
                        <p class="state error">"Erro ao carregar o cardápio: " {e.to_string()}</p>
                    }.into_any(),
                }
            })}
        </Suspense>
    }
}

/// Render do catálogo: meta por tenant (SEO) + header + nav de categorias
/// + grid. Após a hidratação, clicar num chip filtra o grid reativamente
/// (estado puro de UI — sem lógica de negócio, §11). No SSR, `sel=""`
/// (Todos) → todos os produtos saem no HTML inicial (SEO).
#[component]
fn CatalogView(data: CatalogData) -> impl IntoView {
    let nome = data.info.name.clone();
    let desc = format!("Cardápio de {nome} — peça online.");
    // URLs já vêm prontas da API (mídia servida como bytes, não base64).
    let cover = data.info.cover_url.clone();
    let logo = data.info.logo_url.clone();
    // URL absoluta para og:image (crawler não resolve caminho relativo).
    // Preferimos a capa (imagem maior/mais representativa) e caímos no logo.
    let origin = data.site_origin.clone();
    let og_image = cover
        .clone()
        .or_else(|| logo.clone())
        .map(|u| if u.starts_with("http") { u } else { format!("{origin}{u}") });
    let cats = data.categories;
    let banners = data.banners;
    let business_hours = data.business_hours;
    let products = StoredValue::new(data.products);
    // Categoria selecionada ("" = Todos).
    let (sel, set_sel) = signal(String::new());
    // Relógio do cliente (horário de funcionamento da loja).
    let now = expect_context::<Now>();

    view! {
        <Title text=nome.clone()/>
        <Meta name="description" content=desc.clone()/>
        // Open Graph — cada cardápio (subdomínio) tem identidade própria
        // ao ser compartilhado em redes/WhatsApp (AI_RULES §3, SEO).
        <Meta property="og:type" content="website"/>
        <Meta property="og:site_name" content=nome.clone()/>
        <Meta property="og:title" content=nome.clone()/>
        <Meta property="og:description" content=desc.clone()/>
        <Meta property="og:locale" content="pt_BR"/>
        {(!origin.is_empty()).then(|| view! { <Meta property="og:url" content=origin.clone()/> })}
        {og_image.clone().map(|img| view! {
            <Meta property="og:image" content=img.clone()/>
            <Meta name="twitter:image" content=img/>
        })}
        <Meta name="twitter:card" content=if og_image.is_some() { "summary_large_image" } else { "summary" }/>
        <Meta name="twitter:title" content=nome.clone()/>
        <Meta name="twitter:description" content=desc/>

        <header class="store-header">
            {cover.map(|c| view! { <img class="store-cover" src=c alt=""/> })}
            <div class="store-id">
                {logo.map(|l| view! { <img class="store-logo" src=l alt=""/> })}
                <h1 class="store-name">{nome}</h1>
                {move || availability::store_status(
                    &business_hours.hours, &business_hours.store_override, now.0.get(),
                ).map(|(open, label)| view! {
                    <span class="store-status" class:closed=!open>
                        <span class="store-dot"></span>
                        {label}
                    </span>
                })}
                <AccountButton/>
            </div>
        </header>

        <BannerCarousel banners/>

        <nav class="cat-nav" aria-label="Categorias">
            <button
                class="cat-chip"
                class:cat-chip-active=move || sel.get().is_empty()
                on:click=move |_| set_sel.set(String::new())
            >
                "Todos"
            </button>
            {cats.into_iter().map(|c| {
                let id_active = c.id.clone();
                let id_click = c.id.clone();
                view! {
                    <button
                        class="cat-chip"
                        class:cat-chip-active=move || sel.get() == id_active
                        on:click=move |_| set_sel.set(id_click.clone())
                    >
                        {c.name}
                    </button>
                }
            }).collect_view()}
        </nav>

        <section class="catalog">
            {move || {
                let s = sel.get();
                products.with_value(|ps| {
                    let filtered: Vec<_> = ps
                        .iter()
                        .filter(|p| s.is_empty() || p.category_id.as_deref() == Some(s.as_str()))
                        .cloned()
                        .collect();
                    if filtered.is_empty() {
                        view! { <p class="state">"Nenhum produto nesta categoria."</p> }.into_any()
                    } else {
                        view! {
                            <div class="product-grid">
                                {filtered.into_iter()
                                    .map(|p| view! { <ProductCard product=p/> })
                                    .collect_view()}
                            </div>
                        }.into_any()
                    }
                })
            }}
        </section>
    }
}
