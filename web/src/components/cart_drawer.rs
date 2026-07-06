use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::cart::Cart;
use crate::checkout::{self, OrderItemPayload};
use crate::format;
use crate::session::Session;
use super::auth_modal::AuthModal;

/// Carrinho: botão flutuante (quando há itens) + drawer com linhas,
/// quantidade e checkout. Deslogado → abre o login; logado → envia o
/// pedido (a API revalida preços/cupom, §11), limpa o carrinho e mostra
/// a confirmação.
#[component]
pub fn CartDrawer() -> impl IntoView {
    let cart = expect_context::<Cart>();
    let session = expect_context::<Session>();
    let (open, set_open) = signal(false);
    let (auth_open, set_auth_open) = signal(false);
    let (notes, set_notes) = signal(String::new());
    let (coupon, set_coupon) = signal(String::new());
    let (submitting, set_submitting) = signal(false);
    let (error, set_error) = signal(String::new());
    let (confirmation, set_confirmation) = signal(None::<checkout::OrderConfirmation>);

    // Decide no clique: deslogado abre o login; logado envia o pedido.
    let on_checkout = move |_| {
        if !session.is_logged() {
            set_auth_open.set(true);
            return;
        }
        if submitting.get_untracked() {
            return;
        }
        let items: Vec<OrderItemPayload> = cart.0.get_untracked().iter().map(|it| {
            OrderItemPayload {
                product_id: it.product.id.clone(),
                product_name: it.product.name.clone(),
                quantity: it.quantity,
                unit_price: it.unit_price(),
                addons_json: it.addons_json(),
            }
        }).collect();
        if items.is_empty() {
            return;
        }
        let token = session.token().unwrap_or_default();
        let notes_v = notes.get_untracked();
        let coupon_v = coupon.get_untracked();
        set_error.set(String::new());
        set_submitting.set(true);
        spawn_local(async move {
            match checkout::create_order(token, items, notes_v, coupon_v).await {
                Ok(conf) => {
                    cart.clear();
                    set_confirmation.set(Some(conf));
                    set_submitting.set(false);
                }
                Err(e) => {
                    set_error.set(format::server_error(&e.to_string()));
                    set_submitting.set(false);
                }
            }
        });
    };

    view! {
        {move || (cart.count() > 0.0).then(|| view! {
            <button class="cart-fab" on:click=move |_| set_open.set(true)>
                <span class="cart-fab-badge">{format!("{:.0}", cart.count())}</span>
                <span class="cart-fab-label">"Ver carrinho"</span>
                <span class="cart-fab-total">{format::money(cart.total())}</span>
            </button>
        })}

        {move || open.get().then(|| view! {
            <div class="cart-overlay" on:click=move |_| set_open.set(false)></div>
            <aside class="cart-drawer">
                <header class="cart-drawer-head">
                    <h2>"Seu pedido"</h2>
                    <button
                        class="cart-close"
                        on:click=move |_| { set_open.set(false); set_confirmation.set(None); }
                        aria-label="Fechar"
                    >
                        "✕"
                    </button>
                </header>

                {move || match confirmation.get() {
                    Some(conf) => view! {
                        <div class="cart-success">
                            <div class="cart-success-mark">"✓"</div>
                            <h3>"Pedido #" {conf.number} " enviado!"</h3>
                            <p>"Total: " {format::money(conf.total)}</p>
                            <button
                                class="cart-checkout"
                                on:click=move |_| { set_open.set(false); set_confirmation.set(None); }
                            >
                                "Fechar"
                            </button>
                        </div>
                    }.into_any(),
                    None => view! {
                        <div class="cart-items">
                            {move || {
                                let items = cart.0.get();
                                if items.is_empty() {
                                    return view! { <p class="state">"Carrinho vazio."</p> }.into_any();
                                }
                                items.into_iter().enumerate().map(|(idx, item)| {
                                    let name = item.product.name.clone();
                                    let qty = item.quantity;
                                    let sub = item.subtotal();
                                    let addons = item.addons.iter()
                                        .map(|a| a.name.clone())
                                        .collect::<Vec<_>>()
                                        .join(", ");
                                    view! {
                                        <div class="cart-row">
                                            <div class="cart-row-info">
                                                <div class="cart-row-name">{name}</div>
                                                {(!addons.is_empty())
                                                    .then(|| view! { <div class="cart-row-addons">{addons}</div> })}
                                                <div class="cart-row-sub">{format::money(sub)}</div>
                                            </div>
                                            <div class="cart-qty">
                                                <button on:click=move |_| cart.bump(idx, -1.0)>"−"</button>
                                                <span>{format::qty(qty)}</span>
                                                <button on:click=move |_| cart.bump(idx, 1.0)>"+"</button>
                                            </div>
                                        </div>
                                    }
                                }).collect_view().into_any()
                            }}
                        </div>

                        <footer class="cart-drawer-foot">
                            <textarea
                                class="field"
                                placeholder="Observações (opcional)"
                                prop:value=move || notes.get()
                                on:input=move |e| set_notes.set(event_target_value(&e))
                            ></textarea>
                            <input
                                class="field"
                                placeholder="Cupom (opcional)"
                                prop:value=move || coupon.get()
                                on:input=move |e| set_coupon.set(event_target_value(&e))
                            />
                            <div class="cart-total-row">
                                <span>"Total"</span>
                                <strong>{move || format::money(cart.total())}</strong>
                            </div>
                            {move || (!error.get().is_empty())
                                .then(|| view! { <p class="auth-error">{error.get()}</p> })}
                            <button
                                class="cart-checkout"
                                disabled=move || submitting.get()
                                on:click=on_checkout
                            >
                                {move || if !session.is_logged() {
                                    "Entrar para finalizar".to_string()
                                } else if submitting.get() {
                                    "Enviando…".to_string()
                                } else {
                                    "Finalizar pedido".to_string()
                                }}
                            </button>
                        </footer>
                    }.into_any(),
                }}
            </aside>

            {move || auth_open.get().then(|| view! {
                <AuthModal on_close=Callback::new(move |_| set_auth_open.set(false))/>
            })}
        })}
    }
}
