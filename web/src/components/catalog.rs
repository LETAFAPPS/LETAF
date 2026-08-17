use leptos::prelude::*;
use leptos_meta::{Meta, Title};
use leptos_router::hooks::use_navigate;

use crate::api::CatalogData;
use crate::availability::{self, Now};
use super::account_button::{AccountButton, AccountPanelOpen};
use super::account_panel::AccountPanel;
use super::banner_carousel::BannerCarousel;
use super::product_card::ProductCard;
use super::icon::{CategoryIcon, Icon};

/// Escurece (toward_white=false) ou clareia (true) uma cor hex misturando
/// com preto/branco por `factor` (0..1). Usada para derivar `--brand-ink`
/// da cor de marca da empresa. Entrada validada como hex; saída é hex.
pub(crate) fn shade(hex: &str, factor: f32, toward_white: bool) -> String {
    let h = hex.trim().trim_start_matches('#');
    let full = if h.len() == 3 {
        h.chars().flat_map(|c| [c, c]).collect::<String>()
    } else {
        h.to_string()
    };
    let parse = |i: usize| u8::from_str_radix(full.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0);
    let (r, g, b) = (parse(0), parse(2), parse(4));
    let target = if toward_white { 255.0 } else { 0.0 };
    let mix = |c: u8| ((c as f32) * (1.0 - factor) + target * factor).round() as u8;
    format!("#{:02x}{:02x}{:02x}", mix(r), mix(g), mix(b))
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

/// Loja aberta agora? Provido no `App`, preenchido pelo `CatalogView`
/// (horário/override), lido pelo `CartDrawer` para avisar quando fechada.
/// Só exibição — o servidor é a autoridade sobre aceitar o pedido (§11).
#[derive(Clone, Copy)]
pub struct StoreOpen(pub RwSignal<bool>);

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
    // Cor de marca livre escolhida pela empresa (hex). Sobrepõe SÓ o
    // `--brand` do site e deriva `--brand-ink` por esquema (no claro
    // escurece p/ texto/botão AA; no escuro clareia). Neutros/preço seguem
    // do tema. As cores derivadas são hex geradas por nós → o `style` só
    // recebe hex validado (sem injeção de CSS, §11).
    let brand = data.info.brand_color.clone().filter(|c| is_hex(c));
    let style_light = brand
        .as_ref()
        .map(|b| format!("--brand:{b};--brand-ink:{}", shade(b, 0.32, false)))
        .unwrap_or_default();
    let style_dark = brand
        .as_ref()
        .map(|b| format!("--brand:{};--brand-ink:{}", shade(b, 0.16, true), shade(b, 0.45, true)))
        .unwrap_or_default();
    // URLs já vêm prontas da API (mídia servida como bytes, não base64).
    let logo = data.info.logo_url.clone();
    // URL absoluta para og:image (crawler não resolve caminho relativo).
    // A capa foi removida — usa a logo.
    let origin = data.site_origin.clone();
    let og_image = logo
        .clone()
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
    // Carrinho (contador no topo). O botão navega para a tela /carrinho.
    let cart_ctx = expect_context::<crate::cart::Cart>();
    // Sessão + painel da conta (aberto pelos botões Perfil e Pedidos).
    let session = expect_context::<crate::session::Session>();
    let account_panel = expect_context::<AccountPanelOpen>();
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
    // Publica "loja aberta?" para o carrinho avisar quando fechada.
    let store_open = expect_context::<StoreOpen>();
    {
        let bh = business_hours.clone();
        Effect::new(move |_| {
            let open = availability::store_status(&bh.hours, &bh.store_override, now.0.get())
                .map(|(o, _)| o)
                .unwrap_or(true);
            store_open.0.set(open);
        });
    }
    // Modal "loja fechada": ao ENTRAR no site com a loja fechada, avisa uma
    // única vez (após a hidratação; no SSR `now=None` → aberto, sem modal e
    // sem mismatch). Só informativo — o cliente ainda navega o cardápio.
    let (closed_modal, set_closed_modal) = signal(false);
    let closed_label = {
        let bh = business_hours.clone();
        Memo::new(move |_| {
            availability::store_status(&bh.hours, &bh.store_override, now.0.get())
                .map(|(_, label)| label)
                .unwrap_or_default()
        })
    };
    {
        let shown = StoredValue::new(false);
        Effect::new(move |_| {
            if !store_open.0.get() && !shown.get_value() {
                shown.set_value(true);
                set_closed_modal.set(true);
            }
        });
    }

    view! {
        <div
            class="store-root"
            data-theme=theme
            style=move || if scheme.0.get() == "dark" { style_dark.clone() } else { style_light.clone() }
        >
        // Modal informativo de loja fechada (aparece 1x ao entrar).
        {move || closed_modal.get().then(|| view! {
            <div class="modal-overlay" on:click=move |_| set_closed_modal.set(false)>
                <div class="closed-modal" on:click=move |e: leptos::ev::MouseEvent| e.stop_propagation()>
                    <div class="closed-modal-mark" aria-hidden="true"><Icon name="empresa"/></div>
                    <p class="closed-modal-when">{move || closed_label.get()}</p>
                    <p class="closed-modal-sub">"Você pode ver o cardápio, mas não é possível fazer pedidos agora."</p>
                    <button class="closed-modal-btn" on:click=move |_| set_closed_modal.set(false)>"Entendi"</button>
                </div>
            </div>
        })}
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
                        ><Icon name="fechar"/></button>
                    })}
                </div>
                <button
                    type="button"
                    class="cart-toggle"
                    aria-label="Abrir carrinho"
                    on:click=move |_| use_navigate()("/carrinho", Default::default())
                >
                    <Icon name="carrinho"/>
                    {move || (cart_ctx.count() > 0.0).then(|| view! {
                        <span class="cart-toggle-badge">{format!("{:.0}", cart_ctx.count())}</span>
                    })}
                </button>
                <button
                    type="button"
                    class="fav-toggle"
                    class:fav-toggle-on=move || fav_only.get()
                    aria-pressed=move || fav_only.get().to_string()
                    aria-label="Ver favoritos"
                    on:click=move |_| { set_fav_only.update(|v| *v = !*v); set_sel.set(String::new()); }
                >
                    <Icon name="favoritos"/>
                </button>
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
                    {move || if scheme.0.get() == "dark" {
                        view! { <Icon name="modo-claro"/> }.into_any()
                    } else {
                        view! { <Icon name="modo-escuro"/> }.into_any()
                    }}
                </button>
                <AccountButton/>
            </div>
        </header>

        // Barra inferior estilo app (só no mobile): carrinho, favoritos, tema.
        <nav class="mobile-nav" aria-label="Ações">
            <button
                type="button"
                class="mnav-item"
                class:mnav-on=move || sel.get().is_empty() && !fav_only.get()
                on:click=move |_| {
                    set_sel.set(String::new());
                    set_fav_only.set(false);
                }
            >
                <span class="mnav-ico"><Icon name="casa"/></span>
                <span class="mnav-lbl">"Início"</span>
            </button>
            <button
                type="button"
                class="mnav-item"
                class:mnav-on=move || fav_only.get()
                on:click=move |_| { set_fav_only.update(|v| *v = !*v); set_sel.set(String::new()); }
            >
                <span class="mnav-ico"><Icon name="favoritos"/></span>
                <span class="mnav-lbl">"Favoritos"</span>
            </button>
            // Carrinho ao CENTRO (3º de 5), como CARD destacado na cor da marca.
            // O contador fica no canto superior do card (não sobre o ícone).
            <button
                type="button"
                class="mnav-item mnav-cart"
                on:click=move |_| use_navigate()("/carrinho", Default::default())
            >
                {move || (cart_ctx.count() > 0.0).then(|| view! {
                    <span class="cart-toggle-badge">{format!("{:.0}", cart_ctx.count())}</span>
                })}
                <span class="mnav-ico"><Icon name="carrinho"/></span>
                <span class="mnav-lbl">"Carrinho"</span>
            </button>
            // Pedidos (no lugar do Tema, que foi para o topo) — tela dedicada.
            // Deslogado abre a MESMA tela de login do Perfil (/entrar).
            <button
                type="button"
                class="mnav-item"
                on:click=move |_| {
                    let dest = if session.is_logged() { "/pedidos" } else { "/entrar" };
                    use_navigate()(dest, Default::default());
                }
            >
                <span class="mnav-ico"><Icon name="orders"/></span>
                <span class="mnav-lbl">"Pedidos"</span>
            </button>
            // Perfil (à direita) — mesmo botão do topo.
            <AccountButton/>
        </nav>
        // Painel da conta (perfil + pedidos), aberto por Perfil/Pedidos.
        {move || (account_panel.0.get() && session.is_logged()).then(|| view! {
            <AccountPanel on_close=Callback::new(move |_| account_panel.0.set(false))/>
        })}

        <BannerCarousel banners/>

        <nav class="cat-rail" aria-label="Categorias">
            <button
                class="cat-tile"
                class:cat-tile-active=move || sel.get().is_empty() && !fav_only.get()
                on:click=move |_| { set_sel.set(String::new()); set_fav_only.set(false); }
            >
                <span class="cat-ico" aria-hidden="true"><CategoryIcon slug="all"/></span>
                <span class="cat-lbl">"Todos"</span>
            </button>
            {cats.into_iter().map(|c| {
                let id_active = c.id.clone();
                let id_click = c.id.clone();
                let slug = c.icon_name.clone().unwrap_or_default();
                view! {
                    <button
                        class="cat-tile"
                        class:cat-tile-active=move || sel.get() == id_active && !fav_only.get()
                        on:click=move |_| { set_sel.set(id_click.clone()); set_fav_only.set(false); }
                    >
                        <span class="cat-ico" aria-hidden="true"><CategoryIcon slug=slug/></span>
                        <span class="cat-lbl">{c.name}</span>
                    </button>
                }
            }).collect_view()}
        </nav>

        <section class="catalog">
            <h2 class="sr-only">"Cardápio"</h2>
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
                        .filter(|p| {
                            !searching
                                || norm(&p.name).contains(&q)
                                || p.description.as_deref().map(|d| norm(d).contains(&q)).unwrap_or(false)
                        })
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
