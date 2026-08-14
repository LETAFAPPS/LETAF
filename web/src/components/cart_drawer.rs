use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::{use_location, use_navigate};

use crate::cart::Cart;
use crate::checkout::{self, OrderItemPayload};
use crate::format;
use crate::session::Session;
use super::icon::Icon;

/// Sinaliza que o cliente foi ao login PELO carrinho e o checkout deve ser
/// retomado ao voltar logado. Provido no `App`; o `CartDrawer` reabre sozinho
/// quando fica logado com este sinal ligado — sem perder o pedido montado.
#[derive(Clone, Copy)]
pub struct ResumeCheckout(pub RwSignal<bool>);

/// Carrinho: botão flutuante (quando há itens) + drawer com linhas,
/// quantidade e checkout. Deslogado → abre o login; logado → envia o
/// pedido (a API revalida preços/cupom, §11), limpa o carrinho e mostra
/// a confirmação.
#[component]
pub fn CartDrawer() -> impl IntoView {
    let cart = expect_context::<Cart>();
    let session = expect_context::<Session>();
    let resume = expect_context::<ResumeCheckout>();
    let location = use_location();
    let (open, set_open) = signal(false);
    let (notes, set_notes) = signal(String::new());
    let (coupon, set_coupon) = signal(String::new());
    let (submitting, set_submitting) = signal(false);
    let (error, set_error) = signal(String::new());
    let (confirmation, set_confirmation) = signal(None::<checkout::OrderConfirmation>);

    // Fecha o drawer no Esc quando aberto (§3 acessibilidade).
    let esc = leptos::prelude::window_event_listener(leptos::ev::keydown, move |ev| {
        if ev.key() == "Escape" && open.get_untracked() {
            set_open.set(false);
            set_confirmation.set(None);
        }
    });
    on_cleanup(move || esc.remove());
    // Taxa de entrega da loja (0 quando não há cadastro ou fora do
    // catálogo). Só exibição — o total oficial vem do servidor. É um signal
    // provido no `App` e preenchido pelo `CatalogView`, lido reativamente.
    let fee = use_context::<crate::components::catalog::DeliveryFee>()
        .map(|d| d.0)
        .unwrap_or_else(|| RwSignal::new(0.0));
    // Loja aberta? (só exibição; o servidor decide aceitar o pedido, §11.)
    let store_open = use_context::<crate::components::catalog::StoreOpen>()
        .map(|s| s.0)
        .unwrap_or_else(|| RwSignal::new(true));
    // Modalidade e endereço. Antes o checkout web mandava sempre
    // "entrega" (o default do backend) e NENHUM endereço — o operador
    // recebia um pedido de entrega sem para onde entregar.
    let (delivery, set_delivery) = signal(true);
    let (addresses, set_addresses) = signal(Vec::<checkout::AddressOption>::new());
    let (address_id, set_address_id) = signal(String::new());
    let (novo_aberto, set_novo_aberto) = signal(false);
    let (a_rua, set_a_rua) = signal(String::new());
    let (a_numero, set_a_numero) = signal(String::new());
    let (a_bairro, set_a_bairro) = signal(String::new());
    let (a_compl, set_a_compl) = signal(String::new());
    let (a_label, set_a_label) = signal("Casa".to_string());

    // Carrega os endereços salvos quando o cliente está logado.
    let carregar_enderecos = move || {
        if !session.is_logged() { return; }
        spawn_local(async move {
            if let Ok(list) = checkout::list_addresses().await {
                if address_id.get_untracked().is_empty() {
                    if let Some(first) = list.first() {
                        set_address_id.set(first.id.clone());
                    }
                }
                set_novo_aberto.set(list.is_empty());
                set_addresses.set(list);
            }
        });
    };
    Effect::new(move |_| {
        if session.is_logged() {
            carregar_enderecos();
        }
    });

    // Retoma o checkout ao voltar do login: se o cliente foi ao login PELO
    // carrinho (`resume`) e agora está logado, reabre o drawer com tudo
    // preservado (os signals do pedido sobreviveram à navegação SPA porque o
    // drawer nunca é desmontado). Só dispara uma vez (consome o sinal).
    Effect::new(move |_| {
        if resume.0.get() && session.is_logged() {
            resume.0.set(false);
            set_open.set(true);
            carregar_enderecos();
        }
    });

    // Salva um endereço novo e já o seleciona.
    let salvar_endereco = move |_: leptos::ev::MouseEvent| {
        if !session.is_logged() { return; }
        let (rua, num, bairro, compl) = (
            a_rua.get_untracked(),
            a_numero.get_untracked(),
            a_bairro.get_untracked(),
            a_compl.get_untracked(),
        );
        if rua.trim().is_empty() || num.trim().is_empty() || bairro.trim().is_empty() {
            set_error.set("Preencha rua, número e bairro.".into());
            return;
        }
        set_error.set(String::new());
        let label = {
            let l = a_label.get_untracked();
            if l.trim().is_empty() { "Casa".to_string() } else { l }
        };
        spawn_local(async move {
            match checkout::add_address(label, rua, num, bairro, compl).await {
                Ok(novo) => {
                    set_address_id.set(novo.id.clone());
                    set_addresses.update(|v| v.push(novo));
                    set_novo_aberto.set(false);
                    set_a_rua.set(String::new());
                    set_a_numero.set(String::new());
                    set_a_bairro.set(String::new());
                    set_a_compl.set(String::new());
                    set_a_label.set("Casa".to_string());
                }
                Err(e) => set_error.set(e.to_string()),
            }
        });
    };

    // Decide no clique: deslogado vai para a tela de login (/entrar) marcando
    // "retomar checkout" — ao voltar logado o drawer reabre com o pedido
    // intacto; logado envia o pedido. `use_navigate()` é chamado aqui dentro
    // (não capturado) para o handler seguir `Copy` — o closure de render é
    // reativo e reexecuta.
    let on_checkout = move |_| {
        if !session.is_logged() {
            resume.0.set(true);
            set_open.set(false);
            use_navigate()("/entrar", Default::default());
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
        let notes_v = notes.get_untracked();
        let coupon_v = coupon.get_untracked();
        let entrega = delivery.get_untracked();
        let addr = address_id.get_untracked();
        // Espelha a regra do backend para dar o aviso ANTES de enviar —
        // a autoridade continua sendo do servidor (§11).
        if entrega && addr.is_empty() {
            set_error.set("Escolha um endereço de entrega.".into());
            return;
        }
        set_error.set(String::new());
        set_submitting.set(true);
        spawn_local(async move {
            let modalidade = if entrega { "delivery" } else { "pickup" };
            match checkout::create_order(
                items,
                notes_v,
                coupon_v,
                modalidade.to_string(),
                addr,
            )
            .await
            {
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
        {move || (cart.count() > 0.0 && location.pathname.get() != "/entrar").then(|| view! {
            <button
                class="cart-fab"
                on:click=move |_| set_open.set(true)
                aria-haspopup="dialog"
                aria-expanded=move || open.get().to_string()
                aria-label="Ver carrinho"
            >
                <span class="cart-fab-badge">{format!("{:.0}", cart.count())}</span>
                <span class="cart-fab-label">"Ver carrinho"</span>
                <span class="cart-fab-total">{format::money(cart.total())}</span>
            </button>
        })}

        {move || open.get().then(|| view! {
            <div class="cart-overlay" on:click=move |_| set_open.set(false)></div>
            <aside class="cart-drawer" role="dialog" aria-modal="true" aria-labelledby="cart-title">
                <header class="cart-drawer-head">
                    <h2 id="cart-title">"Seu pedido"</h2>
                    <button
                        class="cart-close"
                        on:click=move |_| { set_open.set(false); set_confirmation.set(None); }
                        aria-label="Fechar"
                    >
                        <Icon name="fechar"/>
                    </button>
                </header>

                {move || match confirmation.get() {
                    Some(conf) => view! {
                        <div class="cart-success">
                            <div class="cart-success-mark" aria-hidden="true"><Icon name="sucesso"/></div>
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
                                    let thumb = if item.product.image_urls.is_empty() {
                                        item.product.image_url.clone()
                                    } else {
                                        item.product.image_urls.first().cloned()
                                    };
                                    let addons = item.addons.iter()
                                        .map(|a| a.name.clone())
                                        .collect::<Vec<_>>()
                                        .join(", ");
                                    view! {
                                        <div class="cart-row">
                                            <div class="cart-thumb">
                                                {thumb.map(|src| view! { <img src=src alt="" loading="lazy"/> })}
                                            </div>
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
                                aria-label="Observações do pedido"
                                placeholder="Observações (opcional)"
                                prop:value=move || notes.get()
                                on:input=move |e| set_notes.set(event_target_value(&e))
                            ></textarea>
                            // ── Entrega ou retirada ──
                            <div class="cart-modal-row" role="group" aria-label="Modalidade de entrega">
                                <button
                                    class=move || if delivery.get() { "chip chip-on" } else { "chip" }
                                    aria-pressed=move || delivery.get().to_string()
                                    on:click=move |_| set_delivery.set(true)
                                >"Entrega"</button>
                                <button
                                    class=move || if delivery.get() { "chip" } else { "chip chip-on" }
                                    aria-pressed=move || (!delivery.get()).to_string()
                                    on:click=move |_| set_delivery.set(false)
                                >"Retirar no local"</button>
                            </div>

                            // ── Endereço (só em entrega) ──
                            {move || delivery.get().then(|| view! {
                                <div class="cart-address">
                                    {move || (!addresses.get().is_empty()).then(|| view! {
                                        <select
                                            class="field"
                                            aria-label="Endereço de entrega"
                                            on:change=move |e| set_address_id.set(event_target_value(&e))
                                        >
                                            <For
                                                each=move || addresses.get()
                                                key=|a| a.id.clone()
                                                let:a
                                            >
                                                <option
                                                    value=a.id.clone()
                                                    selected=move || address_id.get() == a.id
                                                >
                                                    {format!("{} — {}", a.label, a.resumo)}
                                                </option>
                                            </For>
                                        </select>
                                    })}
                                    {move || (!novo_aberto.get()).then(|| view! {
                                        <button
                                            class="link-btn"
                                            on:click=move |_| set_novo_aberto.set(true)
                                        >"+ Novo endereço"</button>
                                    })}
                                    {move || novo_aberto.get().then(|| view! {
                                        <div class="cart-address-form">
                                            <input class="field" placeholder="Rótulo (ex.: Casa, Trabalho)" aria-label="Rótulo do endereço"
                                                prop:value=move || a_label.get()
                                                on:input=move |e| set_a_label.set(event_target_value(&e)) />
                                            <input class="field" placeholder="Rua" aria-label="Rua"
                                                prop:value=move || a_rua.get()
                                                on:input=move |e| set_a_rua.set(event_target_value(&e)) />
                                            <input class="field" placeholder="Número" aria-label="Número"
                                                prop:value=move || a_numero.get()
                                                on:input=move |e| set_a_numero.set(event_target_value(&e)) />
                                            <input class="field" placeholder="Bairro" aria-label="Bairro"
                                                prop:value=move || a_bairro.get()
                                                on:input=move |e| set_a_bairro.set(event_target_value(&e)) />
                                            <input class="field" placeholder="Complemento (opcional)" aria-label="Complemento"
                                                prop:value=move || a_compl.get()
                                                on:input=move |e| set_a_compl.set(event_target_value(&e)) />
                                            <button class="link-btn" on:click=salvar_endereco>
                                                "Salvar endereço"
                                            </button>
                                        </div>
                                    })}
                                </div>
                            })}

                            <input
                                class="field"
                                aria-label="Cupom de desconto"
                                placeholder="Cupom (opcional)"
                                prop:value=move || coupon.get()
                                on:input=move |e| set_coupon.set(event_target_value(&e))
                            />
                            {move || (!coupon.get().trim().is_empty()).then(|| view! {
                                <p class="cart-hint">"O desconto do cupom é aplicado na confirmação do pedido."</p>
                            })}
                            // Com taxa de entrega, o cliente vê a composição
                            // (subtotal + taxa) em vez de um total que não
                            // bate com a soma dos itens.
                            {move || (fee.get() > 0.0 && delivery.get()).then(|| view! {
                                <div class="cart-total-row">
                                    <span>"Subtotal"</span>
                                    <span>{format::money(cart.total())}</span>
                                </div>
                                <div class="cart-total-row">
                                    <span>"Taxa de entrega"</span>
                                    <span>{format::money(fee.get())}</span>
                                </div>
                            })}
                            <div class="cart-total-row">
                                <span>"Total"</span>
                                <strong>{move || format::money(cart.total() + if delivery.get() { fee.get() } else { 0.0 })}</strong>
                            </div>
                            {move || (!store_open.get()).then(|| view! {
                                <p class="cart-closed">
                                    "A loja está fechada agora. Você pode montar o pedido, mas ele só será confirmado quando a loja reabrir."
                                </p>
                            })}
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
        })}
    }
}
