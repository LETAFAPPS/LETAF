use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::account;
use crate::components::icon::Icon;
use crate::format;
use crate::session::Session;

/// Painel da conta (logado): editar nome/telefone + histórico de pedidos
/// + sair. Carrega perfil e pedidos via server fn (proxy à API). Só abre
/// no cliente — não há conteúdo de conta no SSR.
#[component]
pub fn AccountPanel(on_close: Callback<()>) -> impl IntoView {
    let session = expect_context::<Session>();

    // Ações autenticadas leem o JWT do cookie HttpOnly no servidor — o cliente
    // não guarda nem envia token (§11).
    let profile = Resource::new(|| (), |_| async move { account::get_profile().await });

    let (name, set_name) = signal(String::new());
    let (phone, set_phone) = signal(String::new());
    let (saved, set_saved) = signal(false);
    let (err, set_err) = signal(String::new());
    let (busy, set_busy) = signal(false);

    // Popula o form quando o perfil carrega.
    Effect::new(move |_| {
        if let Some(Ok(p)) = profile.get() {
            set_name.set(p.name.clone());
            set_phone.set(p.phone.clone().unwrap_or_default());
        }
    });

    let save = Callback::new(move |_: ()| {
        if busy.get_untracked() {
            return;
        }
        set_err.set(String::new());
        set_saved.set(false);
        set_busy.set(true);
        let n = name.get_untracked();
        let p = phone.get_untracked();
        spawn_local(async move {
            match account::update_profile(n, p, String::new(), String::new()).await {
                Ok(info) => {
                    session.rename(info.name.clone());
                    set_saved.set(true);
                    set_busy.set(false);
                    // Feedback de sucesso some sozinho (não fica ambíguo).
                    set_timeout(
                        move || set_saved.set(false),
                        std::time::Duration::from_secs(3),
                    );
                }
                Err(e) => {
                    set_err.set(format::server_error(&e.to_string()));
                    set_busy.set(false);
                }
            }
        });
    });

    // Fecha no Esc (§3 acessibilidade). Ouve na janela; removido no unmount.
    let esc = leptos::prelude::window_event_listener(leptos::ev::keydown, move |ev| {
        match ev.key().as_str() {
            "Escape" => on_close.run(()),
            "Tab" => crate::focus::trap(".account-panel", &ev),
            _ => {}
        }
    });
    on_cleanup(move || esc.remove());
    // Foco: guarda o gatilho, foca o 1º focável ao abrir e devolve ao fechar.
    let trigger = crate::focus::active_element();
    Effect::new(move |_| crate::focus::focus_first(".account-panel"));
    on_cleanup(move || crate::focus::restore(trigger));

    view! {
        <div class="modal-overlay" on:click=move |_| on_close.run(())>
            <div
                class="account-panel"
                role="dialog"
                aria-modal="true"
                aria-labelledby="acc-title"
                on:click=|e: leptos::ev::MouseEvent| e.stop_propagation()
            >
                <header class="pm-head">
                    <h2 class="pm-name" id="acc-title">"Minha conta"</h2>
                    <button class="cart-close" on:click=move |_| on_close.run(()) aria-label="Fechar">
                        <Icon name="fechar"/>
                    </button>
                </header>
                <div class="account-body">
                    <section>
                        <h3 class="acc-section">"Meus dados"</h3>
                        <Suspense fallback=|| view! {
                            <div class="acc-skel">
                                <div class="skeleton skel-input"></div>
                                <div class="skeleton skel-input"></div>
                                <div class="skeleton skel-input"></div>
                                <div class="skeleton skel-btn"></div>
                            </div>
                        }>
                            {move || Suspend::new(async move {
                                match profile.await {
                                    Ok(p) => view! {
                                        <form on:submit=move |ev: leptos::ev::SubmitEvent| { ev.prevent_default(); save.run(()); }>
                                            <input class="field" type="email" aria-label="E-mail" prop:value=p.email disabled=true/>
                                            <input
                                                class="field"
                                                placeholder="Nome"
                                                aria-label="Nome"
                                                prop:value=move || name.get()
                                                on:input=move |e| set_name.set(event_target_value(&e))
                                            />
                                            <input
                                                class="field"
                                                placeholder="Telefone"
                                                aria-label="Telefone"
                                                prop:value=move || phone.get()
                                                on:input=move |e| set_phone.set(event_target_value(&e))
                                            />
                                            {move || (!err.get().is_empty())
                                                .then(|| view! { <p class="auth-error">{err.get()}</p> })}
                                            {move || saved.get()
                                                .then(|| view! { <p class="acc-saved">"Dados salvos!"</p> })}
                                            <button
                                                type="submit"
                                                class="pm-add auth-submit"
                                                disabled=move || busy.get()
                                            >
                                                {move || if busy.get() { "Salvando…" } else { "Salvar" }}
                                            </button>
                                        </form>
                                    }.into_any(),
                                    Err(_) => view! {
                                        <p class="state error">"Não foi possível carregar o perfil."</p>
                                    }.into_any(),
                                }
                            })}
                        </Suspense>
                    </section>

                    <button
                        class="account-btn account-logout"
                        on:click=move |_| {
                            // Limpa a exibição no cliente E apaga o cookie HttpOnly no servidor.
                            session.clear();
                            spawn_local(async { let _ = crate::session::customer_logout().await; });
                            on_close.run(());
                        }
                    >
                        "Sair"
                    </button>
                </div>
            </div>
        </div>
    }
}
