use leptos::prelude::*;
use leptos::task::spawn_local;

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

    let submit = move |_| {
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

    view! {
        <div class="modal-overlay" on:click=move |_| on_close.run(())>
            <div class="auth-modal" on:click=|e: leptos::ev::MouseEvent| e.stop_propagation()>
                <header class="pm-head">
                    <div class="pm-name">
                        {move || if is_register.get() { "Criar conta" } else { "Entrar" }}
                    </div>
                    <button class="cart-close" on:click=move |_| on_close.run(()) aria-label="Fechar">
                        "✕"
                    </button>
                </header>
                <div class="auth-body">
                    {move || is_register.get().then(|| view! {
                        <input
                            class="field"
                            placeholder="Nome"
                            prop:value=move || name.get()
                            on:input=move |e| set_name.set(event_target_value(&e))
                        />
                    })}
                    <input
                        class="field"
                        type="email"
                        placeholder="E-mail"
                        prop:value=move || email.get()
                        on:input=move |e| set_email.set(event_target_value(&e))
                    />
                    {move || is_register.get().then(|| view! {
                        <input
                            class="field"
                            placeholder="Telefone (opcional)"
                            prop:value=move || phone.get()
                            on:input=move |e| set_phone.set(event_target_value(&e))
                        />
                    })}
                    <input
                        class="field"
                        type="password"
                        placeholder="Senha"
                        prop:value=move || password.get()
                        on:input=move |e| set_password.set(event_target_value(&e))
                    />
                    {move || (!error.get().is_empty())
                        .then(|| view! { <p class="auth-error">{error.get()}</p> })}
                    <button class="pm-add auth-submit" disabled=move || busy.get() on:click=submit>
                        {move || if busy.get() {
                            "Aguarde…".to_string()
                        } else if is_register.get() {
                            "Cadastrar".to_string()
                        } else {
                            "Entrar".to_string()
                        }}
                    </button>
                    <button
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
                </div>
            </div>
        </div>
    }
}
