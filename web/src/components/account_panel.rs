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

    // Ações autenticadas leem o JWT do cookie HttpOnly no servidor — o cliente
    // não guarda nem envia token (§11).
    let profile = Resource::new(|| (), |_| async move { account::get_profile().await });
    let orders = Resource::new(|| (), |_| async move { account::list_orders().await });

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
                                                        {format::iso_date_br(&o.created_at)}
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
