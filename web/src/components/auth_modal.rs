use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::components::icon::Icon;
use crate::format;
use crate::session::{self, Session};

/// Modal de login/cadastro do cliente final. Só COLETA credenciais e
/// chama a server fn (que faz proxy à API) — quem valida e emite o JWT
/// é o backend (§11). Em sucesso, guarda a sessão e fecha.
#[component]
pub fn AuthModal(on_close: Callback<()>) -> impl IntoView {
    let session = expect_context::<Session>();
    let (is_register, set_is_register) = signal(false);
    let (name, set_name) = signal(String::new());
    let (email, set_email) = signal(String::new());
    let (phone, set_phone) = signal(String::new());
    let (password, set_password) = signal(String::new());
    let (error, set_error) = signal(String::new());
    let (busy, set_busy) = signal(false);
    // Mostrar/ocultar senha — UI pura (nada sai daqui sem a API).
    let (show_pw, set_show_pw) = signal(false);

    let submit = move || {
        if busy.get_untracked() {
            return;
        }
        set_error.set(String::new());
        set_busy.set(true);
        let reg = is_register.get_untracked();
        let n = name.get_untracked();
        let e = email.get_untracked();
        let p = phone.get_untracked();
        let pw = password.get_untracked();
        spawn_local(async move {
            let res = if reg {
                session::customer_register(n, e, p, pw).await
            } else {
                session::customer_login(e, pw).await
            };
            match res {
                Ok(info) => {
                    session.set(info);
                    on_close.run(());
                }
                Err(err) => {
                    set_error.set(format::server_error(&err.to_string()));
                    set_busy.set(false);
                }
            }
        });
    };

    // Fecha no Esc (§3 acessibilidade). Ouve na janela; handle removido no unmount.
    let esc = leptos::prelude::window_event_listener(leptos::ev::keydown, move |ev| {
        match ev.key().as_str() {
            "Escape" => on_close.run(()),
            "Tab" => crate::focus::trap(".auth-modal", &ev),
            _ => {}
        }
    });
    on_cleanup(move || esc.remove());
    // Foco: guarda o gatilho, foca o 1º focável ao abrir e devolve ao fechar.
    let trigger = crate::focus::active_element();
    Effect::new(move |_| crate::focus::focus_first(".auth-modal"));
    on_cleanup(move || crate::focus::restore(trigger));

    view! {
        <div class="modal-overlay" on:click=move |_| on_close.run(())>
            <div
                class="auth-modal"
                role="dialog"
                aria-modal="true"
                aria-labelledby="auth-title"
                on:click=|e: leptos::ev::MouseEvent| e.stop_propagation()
            >
                <header class="pm-head">
                    <div class="pm-head-text">
                        <h2 class="pm-name" id="auth-title">
                            {move || if is_register.get() { "Criar conta" } else { "Entrar" }}
                        </h2>
                        <div class="pm-desc">
                            {move || if is_register.get() {
                                "Crie sua conta para pedir mais rápido"
                            } else {
                                "Acesse sua conta para pedir mais rápido"
                            }}
                        </div>
                    </div>
                    <button type="button" class="cart-close" on:click=move |_| on_close.run(()) aria-label="Fechar">
                        <Icon name="fechar"/>
                    </button>
                </header>
                <form class="auth-body"
                    on:submit=move |ev: leptos::ev::SubmitEvent| { ev.prevent_default(); submit(); }>
                    {move || is_register.get().then(|| view! {
                        <input
                            class="field"
                            placeholder="Nome"
                            aria-label="Nome"
                            prop:value=move || name.get()
                            on:input=move |e| set_name.set(event_target_value(&e))
                        />
                    })}
                    <input
                        class="field"
                        type="email"
                        placeholder="E-mail"
                        aria-label="E-mail"
                        prop:value=move || email.get()
                        on:input=move |e| set_email.set(event_target_value(&e))
                    />
                    {move || is_register.get().then(|| view! {
                        <input
                            class="field"
                            placeholder="Telefone (opcional)"
                            aria-label="Telefone (opcional)"
                            prop:value=move || phone.get()
                            on:input=move |e| set_phone.set(event_target_value(&e))
                        />
                    })}
                    <div class="pw-wrap">
                        <input
                            class="field"
                            type=move || if show_pw.get() { "text" } else { "password" }
                            placeholder="Senha"
                            aria-label="Senha"
                            prop:value=move || password.get()
                            on:input=move |e| set_password.set(event_target_value(&e))
                        />
                        <button
                            type="button"
                            class="pw-eye"
                            on:click=move |_| set_show_pw.update(|v| *v = !*v)
                            aria-label=move || if show_pw.get() { "Ocultar senha" } else { "Mostrar senha" }
                        >
                            {move || if show_pw.get() {
                                view! { <Icon name="ocultar"/> }.into_any()
                            } else {
                                view! { <Icon name="visualizar"/> }.into_any()
                            }}
                        </button>
                    </div>
                    {move || (!error.get().is_empty())
                        .then(|| view! { <p class="auth-error">{error.get()}</p> })}
                    <button type="submit" class="pm-add auth-submit" disabled=move || busy.get()>
                        {move || if busy.get() {
                            "Aguarde…".to_string()
                        } else if is_register.get() {
                            "Cadastrar".to_string()
                        } else {
                            "Entrar".to_string()
                        }}
                    </button>
                    <button
                        type="button"
                        class="auth-toggle"
                        on:click=move |_| {
                            set_error.set(String::new());
                            set_is_register.update(|v| *v = !*v);
                        }
                    >
                        {move || if is_register.get() {
                            "Já tenho conta — entrar"
                        } else {
                            "Criar uma conta"
                        }}
                    </button>
                </form>
            </div>
        </div>
    }
}
