use leptos::prelude::*;

use crate::components::icon::Icon;
use crate::session::Session;

/// Sinal compartilhado que abre o painel da conta (perfil + "Meus pedidos").
/// Provido no `App`; o painel é renderizado uma única vez no `CatalogView`.
/// Tanto o botão "Perfil" quanto o "Pedidos" apenas ligam este sinal.
#[derive(Clone, Copy)]
pub struct AccountPanelOpen(pub RwSignal<bool>);

/// Botão de PERFIL (ícone de usuário). Deslogado → vai para `/entrar`; logado
/// → abre o painel da conta (via sinal compartilhado). Topo (desktop) e barra
/// inferior (mobile) — a aparência muda por CSS conforme o contexto.
#[component]
pub fn AccountButton() -> impl IntoView {
    let session = expect_context::<Session>();
    let panel = expect_context::<AccountPanelOpen>();

    move || if session.is_logged() {
        view! {
            <button
                class="account-btn"
                on:click=move |_| panel.0.set(true)
                aria-haspopup="dialog"
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
    }
}
