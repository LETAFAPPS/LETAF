//! Favoritos do cliente (preferência de UI) — estado compartilhado por
//! contexto + persistência em localStorage. AI_RULES §1/§11: é só
//! preferência local; nenhuma lógica de negócio nem autoridade no
//! cliente. No SSR o localStorage não existe → favoritos vazios; um
//! `Effect` carrega no cliente após a hidratação (sem mismatch).

use std::collections::HashSet;

use leptos::prelude::RwSignal;

/// Contexto: conjunto de IDs de produtos favoritados.
#[derive(Clone, Copy)]
pub struct Favorites(pub RwSignal<HashSet<String>>);

#[cfg(feature = "hydrate")]
const KEY: &str = "letaf:favorites";

#[cfg(feature = "hydrate")]
pub fn load() -> HashSet<String> {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item(KEY).ok().flatten())
        .and_then(|json| serde_json::from_str::<Vec<String>>(&json).ok())
        .map(|v| v.into_iter().collect())
        .unwrap_or_default()
}

#[cfg(not(feature = "hydrate"))]
pub fn load() -> HashSet<String> {
    HashSet::new()
}

#[cfg(feature = "hydrate")]
pub fn save(favs: &HashSet<String>) {
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let list: Vec<&String> = favs.iter().collect();
        if let Ok(json) = serde_json::to_string(&list) {
            let _ = storage.set_item(KEY, &json);
        }
    }
}

#[cfg(not(feature = "hydrate"))]
pub fn save(_favs: &HashSet<String>) {}
