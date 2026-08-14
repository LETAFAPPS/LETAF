use leptos::prelude::*;

use crate::session::Session;
use super::account_panel::AccountPanel;

/// Área de conta no header: "Entrar" (vai para a tela de login `/entrar`)
/// quando deslogado; "Olá, {nome}" (abre o painel da conta) quando logado.
/// No SSR a sessão é vazia → "Entrar"; após a hidratação reflete o storage.
#[component]
pub fn AccountButton() -> impl IntoView {
    let session = expect_context::<Session>();
    let (panel_open, set_panel_open) = signal(false);

    view! {
        {move || if session.is_logged() {
            let name = session.name().unwrap_or_default();
            view! {
                <button
                    class="account-btn"
                    on:click=move |_| set_panel_open.set(true)
                    aria-haspopup="dialog"
                    aria-expanded=move || panel_open.get().to_string()
                >
                    "Olá, " {name}
                </button>
            }
            .into_any()
        } else {
            view! {
                <a class="account-btn" href="/entrar">"Entrar"</a>
            }
            .into_any()
        }}
        {move || panel_open.get().then(|| view! {
            <AccountPanel on_close=Callback::new(move |_| set_panel_open.set(false))/>
        })}
    }
}
