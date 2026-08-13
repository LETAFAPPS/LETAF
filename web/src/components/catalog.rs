use leptos::prelude::*;
use leptos_meta::{Meta, Title};

use crate::api::CatalogData;
use crate::availability::{self, Now};
use super::account_button::AccountButton;
use super::banner_carousel::BannerCarousel;
use super::product_card::ProductCard;

/// Mapeia o slug do ícone da categoria (allowlist em
/// `core::category::icons`) para um glifo exibido no tile. O backend é a
/// fonte da verdade do slug; cada client escolhe o markup local (aqui,
/// emoji — sem assets, funciona offline). Slug ausente/desconhecido cai
/// no prato genérico.
fn cat_emoji(icon: Option<&str>) -> &'static str {
    match icon {
        Some("ice-cream") => "🍦",
        Some("drink") => "🥤",
        Some("pizza") => "🍕",
        Some("burger") => "🍔",
        Some("combo") => "🍱",
        Some("snack") => "🥟",
        Some("dessert") => "🍰",
        Some("candy") => "🍬",
        Some("coffee") => "☕",
        Some("bread") => "🥖",
        Some("salad") => "🍢",
        Some("meat") => "🥩",
        Some("convenience") => "🏪",
        _ => "🍽️",
    }
}

/// Normaliza texto para busca: minúsculas + sem acentos (pt-BR). Sem isto,
/// "acai"/"pao"/"cafe" não achariam "Açaí"/"Pão"/"Café". É recorte de
/// exibição (§11), não decisão de negócio.
fn norm(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' | 'Á' | 'À' | 'Â' | 'Ã' | 'Ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' | 'Í' | 'Ì' | 'Î' | 'Ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' | 'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' | 'Ú' | 'Ù' | 'Û' | 'Ü' => 'u',
            'ç' | 'Ç' => 'c',
            'ñ' | 'Ñ' => 'n',
            other => other.to_ascii_lowercase(),
        })
        .collect()
}

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
            // Match ANCORADO (via host_is_local): "localhost.evil.com" não é
            // tratado como dev. §11.
            if crate::session::host_is_local(&host) {
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
                    Err(e) => {
                        // Mensagem amigável (sem jargão do ServerFnError) + caminho
                        // de recuperação. Antes exibia o erro técnico cru e sem saída.
                        let detalhe = crate::format::server_error(&e.to_string());
                        view! {
                            <div class="state error">
                                <p>"Não foi possível carregar o cardápio. Verifique a conexão e tente novamente."</p>
                                <p style="font-size:.8rem;opacity:.7;margin:.25rem 0 .9rem">{detalhe}</p>
                                <button class="add-btn" on:click=move |_| catalog.refetch()>"Tentar de novo"</button>
                            </div>
                        }.into_any()
                    },
                }
            })}
        </Suspense>
    }
}

/// Taxa de entrega da loja, disponível para o carrinho via contexto.
///
/// É um `RwSignal` provido no `App` (ancestral comum do `Router` e do
/// `CartDrawer`) e preenchido pelo `CatalogView` quando o catálogo carrega.
/// Antes era `DeliveryFee(f64)` provido DENTRO do `CatalogView`: como o
/// `CartDrawer` é IRMÃO do `Router` (não descendente), o `use_context` dele
/// não enxergava o contexto e a taxa aparecia sempre como 0. Sendo signal,
/// além de visível no escopo certo, a taxa também vira reativa.
#[derive(Clone, Copy)]
pub struct DeliveryFee(pub RwSignal<f64>);

/// Render do catálogo: meta por tenant (SEO) + header + nav de categorias
/// + grid. Após a hidratação, clicar num chip filtra o grid reativamente
/// (estado puro de UI — sem lógica de negócio, §11). No SSR, `sel=""`
/// (Todos) → todos os produtos saem no HTML inicial (SEO).
#[component]
fn CatalogView(data: CatalogData) -> impl IntoView {
    let nome = data.info.name.clone();
    let desc = format!("Cardápio de {nome} — peça online.");
    // Tema do site (slug resolvido do tipo de empresa pelo servidor). Vira
    // `data-theme` no wrapper `.store-root` → preset de CSS (as variáveis
    // cascateiam p/ todo o conteúdo). Aplicado no SSR (SEO/1ª pintura).
    let theme = data.info.theme.clone();
    // Tema padrão do site (claro/escuro) escolhido pela empresa; estado
    // inicial do scheme quando o visitante ainda não escolheu.
    let default_scheme = data.info.default_scheme.clone();
    // Paleta escolhida pela EMPRESA: sobrepõe as cores do tema via `style`
    // inline no wrapper (variáveis CSS; inline vence o preset do tipo). Vazio
    // quando a empresa não escolheu paleta.
    // Defesa-em-profundidade: o servidor já resolve a paleta de um catálogo
    // FIXO, mas o web revalida cada cor como `#RRGGBB`/`#RGB` antes de injetar
    // no atributo `style` — se um dia a API ecoasse cor livre da empresa, um
    // valor malicioso (ex.: `red;} ...`) não viraria injeção de CSS.
    let is_hex = |c: &str| {
        let h = c.strip_prefix('#').unwrap_or("");
        (h.len() == 3 || h.len() == 6) && h.chars().all(|ch| ch.is_ascii_hexdigit())
    };
    let palette = data
        .info
        .palette
        .as_ref()
        .filter(|p| [&p.brand, &p.price, &p.ink, &p.muted, &p.line].iter().all(|c| is_hex(c)));
    // No CLARO a paleta custom carrega tudo (identidade + neutros). No
    // ESCURO carrega só a IDENTIDADE (brand/price); os neutros
    // (ink/muted/line) seguem o scheme escuro, senão texto escuro cairia
    // sobre superfície escura (ilegível). Escolha reativa no `style`.
    let palette_full = palette
        .map(|p| format!(
            "--brand:{};--price:{};--ink:{};--muted:{};--line:{}",
            p.brand, p.price, p.ink, p.muted, p.line
        ))
        .unwrap_or_default();
    let palette_ident = palette
        .map(|p| format!("--brand:{};--price:{}", p.brand, p.price))
        .unwrap_or_default();
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
    // Publica a taxa de entrega no signal compartilhado (provido no `App`,
    // acima do `Router` e do `CartDrawer`): o carrinho a exibe antes do
    // checkout. O total oficial continua sendo do servidor (§11). Só o
    // cliente precisa disso (o SSR renderiza o carrinho vazio), então o
    // Effect roda pós-hidratação, sem risco de mismatch.
    let fee_value = data.info.delivery_fee;
    let delivery_fee = expect_context::<DeliveryFee>();
    Effect::new(move |_| delivery_fee.0.set(fee_value));
    let cats = data.categories;
    let banners = data.banners;
    let business_hours = data.business_hours;
    let products = StoredValue::new(data.products);
    // Categoria selecionada ("" = Todos).
    let (sel, set_sel) = signal(String::new());
    // Busca por nome — filtro puro de apresentação sobre o catálogo
    // PÚBLICO já carregado (mesmo espírito do filtro por categoria).
    // Nenhuma regra/decisão no cliente (§11); só recorta o que exibir.
    let (query, set_query) = signal(String::new());
    // Filtro "só favoritos" (preferência de UI; os favoritos vivem no
    // contexto/localStorage). Recorte de exibição, sem regra de negócio.
    let favs = expect_context::<crate::favorites::Favorites>();
    let (fav_only, set_fav_only) = signal(false);
    // Tema claro/escuro (preferência do usuário; ver `theme.rs`).
    let scheme = expect_context::<crate::theme::Scheme>();
    // Estado inicial do scheme: a escolha salva do visitante PREVALECE; na
    // falta dela usa o padrão da empresa (default_scheme); na falta deste,
    // segue o sistema. Roda uma vez no cliente (get_untracked → sem loop).
    Effect::new(move |_| {
        if !crate::theme::load().is_empty() {
            return; // visitante já escolheu → prevalece
        }
        if !scheme.0.get_untracked().is_empty() {
            return; // já resolvido
        }
        let initial = match default_scheme.as_deref() {
            Some("light") => "light",
            Some("dark") => "dark",
            _ => if crate::theme::prefers_dark() { "dark" } else { "light" },
        };
        scheme.0.set(initial.to_string());
    });
    // Relógio do cliente (horário de funcionamento da loja).
    let now = expect_context::<Now>();

    view! {
        <div
            class="store-root"
            data-theme=theme
            style=move || if scheme.0.get() == "dark" { palette_ident.clone() } else { palette_full.clone() }
        >
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

        <header class="topbar">
            <div class="topbar-inner">
                {logo.map({ let n = nome.clone(); move |l| view! { <img class="topbar-logo" src=l alt=n/> } })}
                <h1 class="topbar-name">{nome}</h1>
                {move || availability::store_status(
                    &business_hours.hours, &business_hours.store_override, now.0.get(),
                ).map(|(open, label)| view! {
                    <span class="store-status" class:closed=!open>
                        <span class="store-dot"></span>
                        {label}
                    </span>
                })}
                <div class="topbar-search">
                    <input
                        class="search-input"
                        type="search"
                        aria-label="Buscar no cardápio"
                        placeholder="O que você quer comer?"
                        prop:value=move || query.get()
                        on:input=move |e| set_query.set(event_target_value(&e))
                    />
                    {move || (!query.get().is_empty()).then(|| view! {
                        <button
                            type="button"
                            class="search-clear"
                            aria-label="Limpar busca"
                            on:click=move |_| set_query.set(String::new())
                        >"✕"</button>
                    })}
                </div>
                <button
                    type="button"
                    class="theme-toggle"
                    on:click=move |_| {
                        let next = if scheme.0.get() == "dark" { "light" } else { "dark" };
                        scheme.0.set(next.to_string());
                        crate::theme::save(next);
                    }
                    aria-label=move || if scheme.0.get() == "dark" {
                        "Mudar para tema claro"
                    } else {
                        "Mudar para tema escuro"
                    }
                >
                    {move || if scheme.0.get() == "dark" { "☀" } else { "🌙" }}
                </button>
                <AccountButton/>
            </div>
        </header>

        {cover.map(|c| view! { <div class="hero-cover"><img src=c alt="" loading="lazy"/></div> })}

        <BannerCarousel banners/>

        <nav class="cat-rail" aria-label="Categorias">
            <button
                class="cat-tile"
                class:cat-tile-active=move || sel.get().is_empty() && !fav_only.get()
                on:click=move |_| { set_sel.set(String::new()); set_fav_only.set(false); }
            >
                <span class="cat-ico" aria-hidden="true">"🍽️"</span>
                <span class="cat-lbl">"Todos"</span>
            </button>
            <button
                class="cat-tile"
                class:cat-tile-active=move || fav_only.get()
                aria-pressed=move || fav_only.get().to_string()
                on:click=move |_| set_fav_only.update(|v| *v = !*v)
            >
                <span class="cat-ico" aria-hidden="true">"♥"</span>
                <span class="cat-lbl">"Favoritos"</span>
            </button>
            {cats.into_iter().map(|c| {
                let id_active = c.id.clone();
                let id_click = c.id.clone();
                let ico = cat_emoji(c.icon_name.as_deref());
                view! {
                    <button
                        class="cat-tile"
                        class:cat-tile-active=move || sel.get() == id_active && !fav_only.get()
                        on:click=move |_| { set_sel.set(id_click.clone()); set_fav_only.set(false); }
                    >
                        <span class="cat-ico" aria-hidden="true">{ico}</span>
                        <span class="cat-lbl">{c.name}</span>
                    </button>
                }
            }).collect_view()}
        </nav>

        <section class="catalog">
            {move || {
                let s = sel.get();
                let q = norm(query.get().trim());
                let searching = !q.is_empty();
                let only_fav = fav_only.get();
                let fav_set = favs.0.get();
                products.with_value(|ps| {
                    let filtered: Vec<_> = ps
                        .iter()
                        .filter(|p| !only_fav || fav_set.contains(&p.id))
                        // Buscando, varre TODAS as categorias; sem busca, respeita
                        // a categoria selecionada.
                        .filter(|p| searching || s.is_empty() || p.category_id.as_deref() == Some(s.as_str()))
                        .filter(|p| !searching || norm(&p.name).contains(&q))
                        .cloned()
                        .collect();
                    if filtered.is_empty() {
                        let msg = if only_fav {
                            "Você ainda não favoritou nenhum item."
                        } else if searching {
                            "Nenhum produto encontrado."
                        } else {
                            "Nenhum produto nesta categoria."
                        };
                        view! { <p class="state">{msg}</p> }.into_any()
                    } else {
                        let count = filtered.len();
                        // Contagem só quando há recorte ativo (busca/favoritos).
                        let show_count = searching || only_fav;
                        view! {
                            {show_count.then(|| view! {
                                <p class="result-count">
                                    {count} {if count == 1 { " item" } else { " itens" }}
                                </p>
                            })}
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
        </div>
    }
}
