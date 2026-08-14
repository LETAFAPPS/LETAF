use leptos::prelude::*;
use leptos_router::hooks::use_navigate;

use crate::account;
use crate::api::CatalogInfo;
use crate::format;
use crate::session::Session;
use crate::theme::Scheme;
use super::icon::Icon;

/// Página "Meus pedidos" (`/pedidos`) — tela separada do perfil. Herda
/// tema/marca do tenant (via /catalog/info) e lista os pedidos do cliente.
#[component]
pub fn OrdersPage() -> impl IntoView {
    let info = Resource::new_blocking(|| (), |_| crate::components::auth_page::get_catalog_info());

    view! {
        <Suspense fallback=|| view! { <p class="state">"Carregando…"</p> }>
            {move || Suspend::new(async move {
                let info = info.await.unwrap_or_default();
                view! { <OrdersView info/> }.into_any()
            })}
        </Suspense>
    }
}

#[component]
fn OrdersView(info: CatalogInfo) -> impl IntoView {
    let session = expect_context::<Session>();
    let scheme = expect_context::<Scheme>();

    let theme = info.theme.clone();
    let default_scheme = info.default_scheme.clone();

    let is_hex = |c: &str| {
        let h = c.strip_prefix('#').unwrap_or("");
        (h.len() == 3 || h.len() == 6) && h.chars().all(|ch| ch.is_ascii_hexdigit())
    };
    let brand = info.brand_color.clone().filter(|c| is_hex(c));
    let style_light = brand
        .as_ref()
        .map(|b| format!("--brand:{b};--brand-ink:{}", super::catalog::shade(b, 0.32, false)))
        .unwrap_or_default();
    let style_dark = brand
        .as_ref()
        .map(|b| format!("--brand:{};--brand-ink:{}", super::catalog::shade(b, 0.16, true), super::catalog::shade(b, 0.45, true)))
        .unwrap_or_default();
    Effect::new(move |_| {
        if !crate::theme::load().is_empty() || !scheme.0.get_untracked().is_empty() {
            return;
        }
        let initial = match default_scheme.as_deref() {
            Some("light") => "light",
            Some("dark") => "dark",
            _ => if crate::theme::prefers_dark() { "dark" } else { "light" },
        };
        scheme.0.set(initial.to_string());
    });

    // Sem login → abre a MESMA tela de login do Perfil (/entrar). Usa
    // `session::load()` (localStorage) direto para não correr com o signal.
    Effect::new(move |_| {
        if crate::session::load().is_none() {
            use_navigate()("/entrar", Default::default());
        }
    });

    // Pedidos do cliente (autenticado via cookie no servidor, §11).
    let orders = Resource::new(|| (), |_| async move { account::list_orders().await });

    view! {
        <div
            class="store-root cart-page"
            data-theme=theme
            style=move || if scheme.0.get() == "dark" { style_dark.clone() } else { style_light.clone() }
        >
            <main class="cart-panel" role="main">
                <header class="cart-drawer-head">
                    <a class="cart-back" href="/" aria-label="Voltar ao cardápio"><Icon name="seta-esquerda"/></a>
                    <h2 id="cart-title">"Meus pedidos"</h2>
                </header>
                <div class="account-body">
                    {move || if !session.is_logged() {
                        // Redirecionando para /entrar (Effect acima).
                        view! { <p class="state">"Carregando…"</p> }.into_any()
                    } else {
                        view! {
                            <Suspense fallback=|| view! {
                                <div class="acc-skel">
                                    <div class="skeleton skel-row"></div>
                                    <div class="skeleton skel-row"></div>
                                    <div class="skeleton skel-row"></div>
                                </div>
                            }>
                                {move || Suspend::new(async move {
                                    match orders.await {
                                        Ok(list) if !list.is_empty() => view! {
                                            <div class="acc-orders">
                                                {list.into_iter().map(|o| view! {
                                                    <div class="acc-order">
                                                        <span class="acc-order-num">"#" {o.number}</span>
                                                        <span class="acc-order-status">{o.status}</span>
                                                        <span class="acc-order-date">{format::iso_date_br(&o.created_at)}</span>
                                                        <span class="acc-order-total">{format::money(o.total)}</span>
                                                    </div>
                                                }).collect_view()}
                                            </div>
                                        }.into_any(),
                                        Ok(_) => view! { <p class="state">"Você ainda não fez pedidos."</p> }.into_any(),
                                        Err(_) => view! { <p class="state error">"Não foi possível carregar os pedidos."</p> }.into_any(),
                                    }
                                })}
                            </Suspense>
                        }.into_any()
                    }}
                </div>
            </main>
        </div>
    }
}
