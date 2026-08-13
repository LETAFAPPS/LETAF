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

/// Prende o Tab DENTRO do diálogo (WCAG 2.4.3): Shift+Tab no primeiro
/// focável volta ao último e Tab no último volta ao primeiro. Chamado no
/// keydown de "Tab" enquanto o modal está aberto.
#[cfg(feature = "hydrate")]
pub fn trap(dialog_selector: &str, ev: &web_sys::KeyboardEvent) {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let sel = format!(
        "{s} button:not([disabled]), {s} [href], {s} input:not([disabled]), \
         {s} select:not([disabled]), {s} textarea:not([disabled]), {s} [tabindex]",
        s = dialog_selector
    );
    let Ok(list) = doc.query_selector_all(&sel) else {
        return;
    };
    let n = list.length();
    if n == 0 {
        return;
    }
    let first = list.get(0).and_then(|node| node.dyn_into::<web_sys::HtmlElement>().ok());
    let last = list
        .get(n - 1)
        .and_then(|node| node.dyn_into::<web_sys::HtmlElement>().ok());
    let (Some(first), Some(last)) = (first, last) else {
        return;
    };
    let active = doc
        .active_element()
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok());
    if ev.shift_key() {
        if active.as_ref() == Some(&first) {
            let _ = last.focus();
            ev.prevent_default();
        }
    } else if active.as_ref() == Some(&last) {
        let _ = first.focus();
        ev.prevent_default();
    }
}

// Genérico no `_ev` para não precisar nomear `web_sys` no build SSR (onde
// a dependência web-sys do crate não está ativa).
#[cfg(not(feature = "hydrate"))]
pub fn trap<E>(_dialog_selector: &str, _ev: &E) {}
