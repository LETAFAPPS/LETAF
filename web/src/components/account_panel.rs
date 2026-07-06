use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::account;
use crate::format;
use crate::session::Session;

/// Painel da conta (logado): editar nome/telefone + histórico de pedidos
/// + sair. Carrega perfil e pedidos via server fn (proxy à API). Só abre
/// no cliente — não há conteúdo de conta no SSR.
#[component]
pub fn AccountPanel(on_close: Callback<()>) -> impl IntoView {
    let session = expect_context::<Session>();
    let token = session.token().unwrap_or_default();

    let tok_p = token.clone();
    let profile = Resource::new(
        || (),
        move |_| {
            let t = tok_p.clone();
            async move { account::get_profile(t).await }
        },
    );
    let tok_o = token.clone();
    let orders = Resource::new(
        || (),
        move |_| {
            let t = tok_o.clone();
            async move { account::list_orders(t).await }
        },
    );

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

    let tok_save = token.clone();
    let save = Callback::new(move |_: ()| {
        if busy.get_untracked() {
            return;
        }
        set_err.set(String::new());
        set_saved.set(false);
        set_busy.set(true);
        let t = tok_save.clone();
        let n = name.get_untracked();
        let p = phone.get_untracked();
        spawn_local(async move {
            match account::update_profile(t, n, p, String::new(), String::new()).await {
                Ok(info) => {
                    session.rename(info.name.clone());
                    set_saved.set(true);
                    set_busy.set(false);
                }
                Err(e) => {
                    set_err.set(format::server_error(&e.to_string()));
                    set_busy.set(false);
                }
            }
        });
    });

    view! {
        <div class="modal-overlay" on:click=move |_| on_close.run(())>
            <div class="account-panel" on:click=|e: leptos::ev::MouseEvent| e.stop_propagation()>
                <header class="pm-head">
                    <div class="pm-name">"Minha conta"</div>
                    <button class="cart-close" on:click=move |_| on_close.run(()) aria-label="Fechar">
                        "✕"
                    </button>
                </header>
                <div class="account-body">
                    <section>
                        <h3 class="acc-section">"Meus dados"</h3>
                        <Suspense fallback=|| view! { <p class="state">"Carregando…"</p> }>
                            {move || Suspend::new(async move {
                                match profile.await {
                                    Ok(p) => view! {
                                        <input class="field" type="email" prop:value=p.email disabled=true/>
                                        <input
                                            class="field"
                                            placeholder="Nome"
                                            prop:value=move || name.get()
                                            on:input=move |e| set_name.set(event_target_value(&e))
                                        />
                                        <input
                                            class="field"
                                            placeholder="Telefone"
                                            prop:value=move || phone.get()
                                            on:input=move |e| set_phone.set(event_target_value(&e))
                                        />
                                        {move || (!err.get().is_empty())
                                            .then(|| view! { <p class="auth-error">{err.get()}</p> })}
                                        {move || saved.get()
                                            .then(|| view! { <p class="acc-saved">"Dados salvos!"</p> })}
                                        <button
                                            class="pm-add auth-submit"
                                            disabled=move || busy.get()
                                            on:click=move |_| save.run(())
                                        >
                                            {move || if busy.get() { "Salvando…" } else { "Salvar" }}
                                        </button>
                                    }.into_any(),
                                    Err(_) => view! {
                                        <p class="state error">"Não foi possível carregar o perfil."</p>
                                    }.into_any(),
                                }
                            })}
                        </Suspense>
                    </section>

                    <section>
                        <h3 class="acc-section">"Meus pedidos"</h3>
                        <Suspense fallback=|| view! { <p class="state">"Carregando…"</p> }>
                            {move || Suspend::new(async move {
                                match orders.await {
                                    Ok(list) if !list.is_empty() => view! {
                                        <div class="acc-orders">
                                            {list.into_iter().map(|o| view! {
                                                <div class="acc-order">
                                                    <span class="acc-order-num">"#" {o.number}</span>
                                                    <span class="acc-order-status">{o.status}</span>
                                                    <span class="acc-order-date">
                                                        {o.created_at.chars().take(10).collect::<String>()}
                                                    </span>
                                                    <span class="acc-order-total">{format::money(o.total)}</span>
                                                </div>
                                            }).collect_view()}
                                        </div>
                                    }.into_any(),
                                    Ok(_) => view! {
                                        <p class="state">"Você ainda não fez pedidos."</p>
                                    }.into_any(),
                                    Err(_) => view! {
                                        <p class="state error">"Não foi possível carregar os pedidos."</p>
                                    }.into_any(),
                                }
                            })}
                        </Suspense>
                    </section>

                    <button
                        class="account-btn account-logout"
                        on:click=move |_| { session.clear(); on_close.run(()); }
                    >
                        "Sair"
                    </button>
                </div>
            </div>
        </div>
    }
}
