use leptos::prelude::*;

use crate::components::icon::Icon;
use crate::session::Session;
use super::account_panel::AccountPanel;

/// Botão de PERFIL (ícone de usuário). Deslogado → vai para a tela de login
/// (`/entrar`); logado → abre o painel da conta. Usado no topo (desktop) e na
/// barra inferior (mobile) — a aparência muda por CSS conforme o contexto.
#[component]
pub fn AccountButton() -> impl IntoView {
    let session = expect_context::<Session>();
    let (panel_open, set_panel_open) = signal(false);

    view! {
        {move || if session.is_logged() {
            view! {
                <button
                    class="account-btn"
                    on:click=move |_| set_panel_open.set(true)
                    aria-haspopup="dialog"
                    aria-expanded=move || panel_open.get().to_string()
                    aria-label="Minha conta"
                >
                    <Icon name="usuario"/>
                    <span class="account-lbl">"Perfil"</span>
                </button>
            }
            .into_any()
        } else {
            view! {
                <a class="account-btn" href="/entrar" aria-label="Perfil">
                    <Icon name="usuario"/>
                    <span class="account-lbl">"Perfil"</span>
                </a>
            }
            .into_any()
        }}
        {move || panel_open.get().then(|| view! {
            <AccountPanel on_close=Callback::new(move |_| set_panel_open.set(false))/>
        })}
    }
}
