use leptos::prelude::*;
use leptos_meta::{provide_meta_context, MetaTags, Stylesheet, Title};
use leptos_router::{
    components::{Route, Router, Routes},
    StaticSegment,
};

use crate::availability::Now;
use crate::cart::Cart;
use crate::components::cart_drawer::CartDrawer;
use crate::components::catalog::CatalogPage;
use crate::favorites::Favorites;
use crate::session::Session;

/// Shell HTML do SSR — injeta scripts de hidratação e meta tags.
pub fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="pt-BR">
            <head>
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <AutoReload options=options.clone() />
                <HydrationScripts options/>
                <MetaTags/>
            </head>
            <body>
                <App/>
            </body>
        </html>
    }
}

/// Componente raiz: contexto de meta + roteador.
#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    // Favoritos compartilhados (preferência do cliente). Nascem vazios —
    // igual ao SSR, que não tem localStorage. O Effect roda só no cliente,
    // após a hidratação, e carrega o localStorage → sem mismatch.
    let favorites = Favorites(RwSignal::new(std::collections::HashSet::new()));
    provide_context(favorites);
    Effect::new(move |_| favorites.0.set(crate::favorites::load()));

    // Carrinho compartilhado (mesmo padrão: nasce vazio, carrega do
    // localStorage no cliente após a hidratação).
    let cart = Cart(RwSignal::new(Vec::new()));
    provide_context(cart);
    Effect::new(move |_| cart.0.set(crate::cart::load()));

    // Sessão do cliente (token JWT). Nasce vazia; o Effect carrega o
    // localStorage no cliente após a hidratação.
    let session = Session(RwSignal::new(None));
    provide_context(session);
    Effect::new(move |_| session.0.set(crate::session::load()));

    // Relógio do cliente p/ horário de funcionamento. Nasce `None` (SSR
    // = tudo aberto/disponível); o Effect lê o navegador na hidratação e
    // reavalia a cada 60s (status acompanha o relógio).
    let now = Now(RwSignal::new(None));
    provide_context(now);
    Effect::new(move |_| {
        now.0.set(crate::availability::browser_now());
        set_interval(
            move || now.0.set(crate::availability::browser_now()),
            std::time::Duration::from_secs(60),
        );
    });

    view! {
        <Stylesheet id="leptos" href="/pkg/letaf-web.css"/>
        <Title text="Cardápio"/>
        <Router>
            <main>
                <Routes fallback=|| "Página não encontrada.".into_view()>
                    <Route path=StaticSegment("") view=CatalogPage/>
                </Routes>
            </main>
        </Router>
        <CartDrawer/>
    }
}
