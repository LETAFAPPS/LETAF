//! Cardápio digital (cliente final) — Leptos com SSR + hidratação.
//!
//! Regras (AI_RULES.md §1, §3, §8, §11): UI 100% Rust em Leptos; o SSR
//! renderiza apenas conteúdo PÚBLICO (catálogo) buscado na API REST
//! (nunca no banco — frontend burro); login/carrinho/checkout seguem
//! thin-client → API com JWT. Empresa identificada pelo subdomínio (`Host`).

pub mod account;
pub mod api;
pub mod app;
pub mod availability;
pub mod cart;
pub mod checkout;
pub mod components;
pub mod discount;
pub mod favorites;
pub mod format;
pub mod session;

/// Cliente HTTP compartilhado do SSR (server fns + proxy de mídia). Reusa o
/// pool de conexões/TLS entre requisições (§13) em vez de recriar por chamada.
#[cfg(feature = "ssr")]
pub fn http_client() -> &'static reqwest::Client {
    use std::sync::OnceLock;
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new)
}

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_body(crate::app::App);
}
