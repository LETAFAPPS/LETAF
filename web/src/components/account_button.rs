use leptos::prelude::*;

use crate::session::Session;
use super::account_panel::AccountPanel;
use super::auth_modal::AuthModal;

/// Área de conta no header: "Entrar" (abre o login) quando deslogado;
/// "Olá, {nome}" (abre o painel da conta) quando logado. No SSR a sessão
/// é vazia → "Entrar"; após a hidratação reflete o localStorage.
#[component]
pub fn AccountButton() -> impl IntoView {
    let session = expect_context::<Session>();
    let (auth_open, set_auth_open) = signal(false);
    let (panel_open, set_panel_open) = signal(false);

    view! {
        {move || if session.is_logged() {
            let name = session.name().unwrap_or_default();
            view! {
                <button class="account-btn" on:click=move |_| set_panel_open.set(true)>
                    "Olá, " {name}
                </button>
            }
            .into_any()
        } else {
            view! {
                <button class="account-btn" on:click=move |_| set_auth_open.set(true)>
                    "Entrar"
                </button>
            }
            .into_any()
        }}
        {move || auth_open.get().then(|| view! {
            <AuthModal on_close=Callback::new(move |_| set_auth_open.set(false))/>
        })}
        {move || panel_open.get().then(|| view! {
            <AccountPanel on_close=Callback::new(move |_| set_panel_open.set(false))/>
        })}
    }
}
