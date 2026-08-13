//! Gestão de foco dos diálogos (§3 acessibilidade): foca o primeiro
//! elemento focável ao ABRIR e devolve o foco ao gatilho ao FECHAR — para
//! quem navega por teclado/leitor de tela não "perder" o foco atrás do
//! overlay. UI pura (§11). No SSR não há DOM → funções viram no-op.

#[cfg(feature = "hydrate")]
use wasm_bindgen::JsCast;

/// Elemento com foco agora (o gatilho que abriu o diálogo). Guardar ao
/// montar o modal e devolver no fechamento.
#[cfg(feature = "hydrate")]
pub fn active_element() -> Option<web_sys::HtmlElement> {
    web_sys::window()?
        .document()?
        .active_element()?
        .dyn_into::<web_sys::HtmlElement>()
        .ok()
}

#[cfg(not(feature = "hydrate"))]
pub fn active_element() -> Option<()> {
    None
}

/// Foca o primeiro focável DENTRO do diálogo (por seletor de classe).
#[cfg(feature = "hydrate")]
pub fn focus_first(dialog_selector: &str) {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let sel = format!(
        "{s} button, {s} [href], {s} input, {s} select, {s} textarea, {s} [tabindex]",
        s = dialog_selector
    );
    if let Ok(Some(el)) = doc.query_selector(&sel) {
        if let Ok(h) = el.dyn_into::<web_sys::HtmlElement>() {
            let _ = h.focus();
        }
    }
}

#[cfg(not(feature = "hydrate"))]
pub fn focus_first(_dialog_selector: &str) {}

/// Devolve o foco ao elemento guardado (gatilho).
#[cfg(feature = "hydrate")]
pub fn restore(el: Option<web_sys::HtmlElement>) {
    if let Some(h) = el {
        let _ = h.focus();
    }
}

#[cfg(not(feature = "hydrate"))]
pub fn restore(_el: Option<()>) {}
