use leptos::prelude::*;
use leptos_meta::{provide_meta_context, MetaTags, Stylesheet, Title};
use leptos_router::{
    components::{Route, Router, Routes},
    StaticSegment,
};

use crate::availability::Now;
use crate::cart::Cart;
use crate::components::cart_drawer::CartDrawer;
use crate::components::catalog::{CatalogPage, DeliveryFee};
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
                // Aplica a preferência de tema salva ANTES da 1ª pintura (sem
                // flash claro→escuro para quem já escolheu). Só a escolha
                // explícita do usuário; sistema (prefers-color-scheme) já é
                // tratado pelo CSS, sem flash.
                <script inner_html="try{var s=localStorage.getItem('letaf:scheme');if(s==='dark'||s==='light')document.documentElement.setAttribute('data-scheme',s);}catch(e){}"></script>
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

    // Taxa de entrega compartilhada: provida AQUI (ancestral comum do
    // `Router` e do `CartDrawer`), nasce 0.0 e é preenchida pelo
    // `CatalogView` quando o catálogo carrega. O `CartDrawer` a lê
    // reativamente. Sem isto, o contexto vivia dentro do `Router` e o
    // `CartDrawer` (irmão) nunca o via → taxa sempre 0.
    provide_context(DeliveryFee(RwSignal::new(0.0)));
    // Loja aberta? (default aberta no SSR; o CatalogView atualiza no cliente.)
    provide_context(crate::components::catalog::StoreOpen(RwSignal::new(true)));
    // Sinaliza "retomar o checkout": marcado quando o cliente vai ao login a
    // partir do carrinho; o `CartDrawer` reabre ao voltar logado, sem perder
    // nada do pedido (itens/observações/cupom/entrega ficam nos signals, que
    // sobrevivem à navegação SPA porque o drawer nunca é desmontado).
    provide_context(crate::components::cart_drawer::ResumeCheckout(RwSignal::new(false)));
    // Abrir o carrinho a partir do botão de carrinho no topo.
    provide_context(crate::components::cart_drawer::CartOpen(RwSignal::new(false)));

    // Tema claro/escuro (preferência do cliente). Nasce vazio (SSR usa o
    // tema padrão pelo CSS). No cliente: usa a escolha salva; se não há,
    // segue o sistema (prefers-color-scheme). A escolha do usuário
    // PREVALECE (§11 — só preferência de UI, sem autoridade).
    let scheme = crate::theme::Scheme(RwSignal::new(String::new()));
    provide_context(scheme);
    // Aplica a ESCOLHA salva do usuário (prevalece). Se não houver, deixa
    // vazio — o `CatalogView` resolve o inicial pelo padrão da empresa
    // (default_scheme) e, na falta dele, pelo sistema.
    Effect::new(move |_| {
        let saved = crate::theme::load();
        if !saved.is_empty() {
            scheme.0.set(saved);
        }
    });
    // Aplica no <html data-scheme> sempre que o esquema muda (toggle).
    Effect::new(move |_| {
        let v = scheme.0.get();
        if !v.is_empty() {
            crate::theme::apply(&v);
        }
    });

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
                    <Route path=StaticSegment("entrar") view=crate::components::auth_page::AuthPage/>
                </Routes>
            </main>
            // Dentro do `Router` (mas FORA das `Routes`): persiste entre rotas
            // e ainda ganha contexto de rota (navegar/ler a URL). Assim o
            // checkout sobrevive ao ir ao login e voltar.
            <CartDrawer/>
        </Router>
    }
}
