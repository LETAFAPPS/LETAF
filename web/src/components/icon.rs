//! Ícones do SISTEMA no cardápio (nada de emoji). AI_RULES §8: fonte única —
//! os mesmos SVGs do desktop (`desktop/ui/assets/icons`), copiados em
//! `web/public/icons`.
//!
//! Dois tipos:
//! - UI (favorito, fechar, sol/lua, …): vetores pequenos com
//!   `stroke="currentColor"` → INLINE (`inner_html`), herdam a cor do
//!   contexto (funciona no claro e no escuro).
//! - Categoria: SVGs raster (PNG embutido), pesados e de cor fixa →
//!   servidos como asset e tingidos via CSS `mask` + `currentColor`.

use leptos::prelude::*;

/// Markup inline de um ícone de UI (vetor). Vazio se desconhecido.
fn ui_svg(name: &str) -> &'static str {
    match name {
        "favorito" => include_str!("../../public/icons/favorito.svg"),
        "favoritos" => include_str!("../../public/icons/favoritos.svg"),
        "fechar" => include_str!("../../public/icons/fechar.svg"),
        "modo-claro" => include_str!("../../public/icons/modo-claro.svg"),
        "modo-escuro" => include_str!("../../public/icons/modo-escuro.svg"),
        "sucesso" => include_str!("../../public/icons/sucesso.svg"),
        "seta-esquerda" => include_str!("../../public/icons/seta-esquerda.svg"),
        "seta-direita" => include_str!("../../public/icons/seta-direita.svg"),
        "visualizar" => include_str!("../../public/icons/visualizar.svg"),
        "ocultar" => include_str!("../../public/icons/ocultar.svg"),
        "busca" => include_str!("../../public/icons/busca.svg"),
        "email" => include_str!("../../public/icons/email.svg"),
        "usuario" => include_str!("../../public/icons/usuario.svg"),
        "telefone" => include_str!("../../public/icons/telefone.svg"),
        "cadeado-aberto" => include_str!("../../public/icons/cadeado-aberto.svg"),
        "confirmar" => include_str!("../../public/icons/confirmar.svg"),
        "empresa" => include_str!("../../public/icons/empresa.svg"),
        _ => "",
    }
}

/// Ícone de UI: SVG inline que herda a cor do contexto (`currentColor`).
#[component]
pub fn Icon(#[prop(into)] name: String) -> impl IntoView {
    view! { <span class="ico" inner_html=ui_svg(&name)></span> }
}

/// Slug de categoria → arquivo SVG servido (valida contra a allowlist para
/// não montar caminho a partir de valor cru; slug desconhecido → placeholder).
fn cat_file(slug: &str) -> &'static str {
    match slug {
        "ice-cream" => "ice-cream",
        "drink" => "drink",
        "pizza" => "pizza",
        "burger" => "burger",
        "combo" => "combo",
        "snack" => "snack",
        "dessert" => "dessert",
        "candy" => "candy",
        "coffee" => "coffee",
        "bread" => "bread",
        "salad" => "salad",
        "meat" => "meat",
        "convenience" => "convenience",
        "all" => "all",
        _ => "placeholder",
    }
}

/// Ícone de categoria: SVG do sistema tingido via `mask` + `currentColor`.
#[component]
pub fn CategoryIcon(#[prop(into)] slug: String) -> impl IntoView {
    let src = format!("/icons/categories/{}.svg", cat_file(&slug));
    let style = format!("-webkit-mask-image:url({src});mask-image:url({src})");
    view! { <span class="cat-svg" style=style></span> }
}
