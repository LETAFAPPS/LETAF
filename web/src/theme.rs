//! Preferência de tema claro/escuro do cliente (UI pura). AI_RULES
//! §1/§11: é só preferência local; nenhuma lógica de negócio nem
//! autoridade no cliente. No SSR não há `localStorage`/`matchMedia`
//! (scheme vazio → tema padrão pelo CSS); um `Effect` resolve no cliente
//! após a hidratação. A escolha do usuário PREVALECE sobre o padrão.

use leptos::prelude::RwSignal;

/// Contexto: esquema de cor efetivo ("dark" | "light" | "" = ainda não
/// resolvido, SSR). Escrito no `<html data-scheme>` para re-tematizar a
/// página inteira (inclusive o `body`).
#[derive(Clone, Copy)]
pub struct Scheme(pub RwSignal<String>);

#[cfg(feature = "hydrate")]
const KEY: &str = "letaf:scheme";

/// Escolha salva pelo usuário ("dark"/"light") ou vazio se nunca trocou.
#[cfg(feature = "hydrate")]
pub fn load() -> String {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item(KEY).ok().flatten())
        .filter(|v| v == "dark" || v == "light")
        .unwrap_or_default()
}

#[cfg(not(feature = "hydrate"))]
pub fn load() -> String {
    String::new()
}

/// Persiste a escolha do usuário (passa a prevalecer sobre o padrão).
#[cfg(feature = "hydrate")]
pub fn save(scheme: &str) {
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = storage.set_item(KEY, scheme);
    }
}

#[cfg(not(feature = "hydrate"))]
pub fn save(_scheme: &str) {}

/// `true` se o sistema pede tema escuro (`prefers-color-scheme: dark`).
#[cfg(feature = "hydrate")]
pub fn prefers_dark() -> bool {
    web_sys::window()
        .and_then(|w| w.match_media("(prefers-color-scheme: dark)").ok().flatten())
        .map(|m| m.matches())
        .unwrap_or(false)
}

#[cfg(not(feature = "hydrate"))]
pub fn prefers_dark() -> bool {
    false
}

/// Aplica o esquema no `<html data-scheme>` (fonte do CSS de dark).
#[cfg(feature = "hydrate")]
pub fn apply(scheme: &str) {
    if let Some(el) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
    {
        let _ = el.set_attribute("data-scheme", scheme);
    }
}

#[cfg(not(feature = "hydrate"))]
pub fn apply(_scheme: &str) {}
