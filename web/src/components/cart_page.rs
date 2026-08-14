use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;

use crate::api::CatalogInfo;
use crate::cart::Cart;
use crate::checkout::{self, OrderItemPayload};
use crate::format;
use crate::session::Session;
use crate::theme::Scheme;
use super::icon::Icon;

/// Sinaliza que o cliente foi ao login PELO carrinho e o checkout deve ser
/// retomado ao voltar logado. Provido no `App`; a `AuthPage` volta para
/// `/carrinho` (em vez de `/`) quando este sinal está ligado.
#[derive(Clone, Copy)]
pub struct ResumeCheckout(pub RwSignal<bool>);

/// Página do carrinho (`/carrinho`) — TELA separada (não é drawer). Busca a
/// info pública do tenant (tema/marca/taxa de entrega) e renderiza itens +
/// checkout. Frontend burro: a API revalida preços/cupom/total (§11).
#[component]
pub fn CartPage() -> impl IntoView {
    let info = Resource::new_blocking(|| (), |_| crate::components::auth_page::get_catalog_info());

    view! {
        <Suspense fallback=|| view! { <p class="state">"Carregando…"</p> }>
            {move || Suspend::new(async move {
                let info = info.await.unwrap_or_default();
                view! { <CartView info/> }.into_any()
            })}
        </Suspense>
    }
}

#[component]
fn CartView(info: CatalogInfo) -> impl IntoView {
    let cart = expect_context::<Cart>();
    let session = expect_context::<Session>();
    let resume = expect_context::<ResumeCheckout>();
    let scheme = expect_context::<Scheme>();

    let theme = info.theme.clone();
    let default_scheme = info.default_scheme.clone();
    let fee = info.delivery_fee;

    // Tema/marca do tenant (igual à tela de login).
    let is_hex = |c: &str| {
        let h = c.strip_prefix('#').unwrap_or("");
        (h.len() == 3 || h.len() == 6) && h.chars().all(|ch| ch.is_ascii_hexdigit())
    };
    let brand = info.brand_color.clone().filter(|c| is_hex(c));
    let style_light = brand
        .as_ref()
        .map(|b| format!("--brand:{b};--brand-ink:{}", super::catalog::shade(b, 0.32, false)))
        .unwrap_or_default();
    let style_dark = brand
        .as_ref()
        .map(|b| format!("--brand:{};--brand-ink:{}", super::catalog::shade(b, 0.16, true), super::catalog::shade(b, 0.45, true)))
        .unwrap_or_default();
    Effect::new(move |_| {
        if !crate::theme::load().is_empty() || !scheme.0.get_untracked().is_empty() {
            return;
        }
        let initial = match default_scheme.as_deref() {
            Some("light") => "light",
            Some("dark") => "dark",
            _ => if crate::theme::prefers_dark() { "dark" } else { "light" },
        };
        scheme.0.set(initial.to_string());
    });

    let (notes, set_notes) = signal(String::new());
    let (coupon, set_coupon) = signal(String::new());
    let (submitting, set_submitting) = signal(false);
    let (error, set_error) = signal(String::new());
    let (confirmation, set_confirmation) = signal(None::<checkout::OrderConfirmation>);

    let (delivery, set_delivery) = signal(true);
    let (addresses, set_addresses) = signal(Vec::<checkout::AddressOption>::new());
    let (address_id, set_address_id) = signal(String::new());
    let (novo_aberto, set_novo_aberto) = signal(false);
    let (a_rua, set_a_rua) = signal(String::new());
    let (a_numero, set_a_numero) = signal(String::new());
    let (a_bairro, set_a_bairro) = signal(String::new());
    let (a_compl, set_a_compl) = signal(String::new());
    let (a_label, set_a_label) = signal("Casa".to_string());

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

    // Deslogado → vai para /entrar marcando "retomar"; ao voltar logado a
    // AuthPage traz de volta para /carrinho. Logado → envia o pedido.
    let on_checkout = move |_| {
        if !session.is_logged() {
            resume.0.set(true);
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
        if entrega && addr.is_empty() {
            set_error.set("Escolha um endereço de entrega.".into());
            return;
        }
        set_error.set(String::new());
        set_submitting.set(true);
        spawn_local(async move {
            let modalidade = if entrega { "delivery" } else { "pickup" };
            match checkout::create_order(items, notes_v, coupon_v, modalidade.to_string(), addr).await {
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
        <div
            class="store-root cart-page"
            data-theme=theme
            style=move || if scheme.0.get() == "dark" { style_dark.clone() } else { style_light.clone() }
        >
            <main class="cart-panel" role="main">
                <header class="cart-drawer-head">
                    <a class="cart-back" href="/" aria-label="Voltar ao cardápio"><Icon name="seta-esquerda"/></a>
                    <h2 id="cart-title">"Seu pedido"</h2>
                </header>

                {move || match confirmation.get() {
                    Some(conf) => view! {
                        <div class="cart-success">
                            <div class="cart-success-mark" aria-hidden="true"><Icon name="sucesso"/></div>
                            <h3>"Pedido #" {conf.number} " enviado!"</h3>
                            <p>"Total: " {format::money(conf.total)}</p>
                            <a class="cart-checkout" href="/">"Voltar ao cardápio"</a>
                        </div>
                    }.into_any(),
                    None => view! {
                        {move || {
                            let items = cart.0.get();
                            if items.is_empty() {
                                return view! {
                                    <div class="cart-empty">
                                        <div class="cart-empty-mark" aria-hidden="true"><Icon name="carrinho"/></div>
                                        <h3>"Seu carrinho está vazio"</h3>
                                        <p>"Adicione itens do cardápio para começar seu pedido."</p>
                                        <a class="cart-checkout" href="/">"Ver cardápio"</a>
                                    </div>
                                }.into_any();
                            }
                            let rows = items.into_iter().enumerate().map(|(idx, item)| {
                                    let name = item.product.name.clone();
                                    let qty = item.quantity;
                                    let unit = item.unit_price();
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
                                                {match thumb {
                                                    Some(src) => view! { <img src=src alt="" loading="lazy"/> }.into_any(),
                                                    None => view! { <span class="cart-thumb-ph" aria-hidden="true"><Icon name="imagem"/></span> }.into_any(),
                                                }}
                                            </div>
                                            <div class="cart-row-info">
                                                <div class="cart-row-name">{name}</div>
                                                <div class="cart-row-addons">
                                                    {if addons.is_empty() { format::qty(qty) } else { addons }}
                                                </div>
                                                <div class="cart-row-priceline">
                                                    <span class="cart-row-price">{format::money(unit)}</span>
                                                    <div class="cart-qty">
                                                        <button aria-label="Diminuir" on:click=move |_| cart.bump(idx, -1.0)>"−"</button>
                                                        <span>{format::qty(qty)}</span>
                                                        <button aria-label="Aumentar" on:click=move |_| cart.bump(idx, 1.0)>"+"</button>
                                                    </div>
                                                </div>
                                            </div>
                                            <button
                                                class="cart-del"
                                                aria-label="Remover item"
                                                on:click=move |_| cart.bump(idx, -qty)
                                            >
                                                <Icon name="lixeira"/>
                                            </button>
                                        </div>
                                    }
                            }).collect_view();
                            view! {
                            <div class="cart-items">{rows}</div>

                            <footer class="cart-drawer-foot">
                            // ── Como receber ──
                            <div class="cart-sec">
                                <label class="cart-sec-title">"Como você quer receber?"</label>
                                <div class="cart-seg" role="group" aria-label="Modalidade de entrega">
                                    <button
                                        class:cart-seg-on=move || delivery.get()
                                        aria-pressed=move || delivery.get().to_string()
                                        on:click=move |_| set_delivery.set(true)
                                    >"Entrega"</button>
                                    <button
                                        class:cart-seg-on=move || !delivery.get()
                                        aria-pressed=move || (!delivery.get()).to_string()
                                        on:click=move |_| set_delivery.set(false)
                                    >"Retirar no local"</button>
                                </div>
                            </div>

                            // ── Endereço (só em entrega) ──
                            {move || delivery.get().then(|| view! {
                                <div class="cart-sec">
                                    <label class="cart-sec-title">"Endereço de entrega"</label>
                                    <div class="cart-address">
                                        {move || (!addresses.get().is_empty()).then(|| view! {
                                            <select
                                                class="field"
                                                aria-label="Endereço de entrega"
                                                on:change=move |e| set_address_id.set(event_target_value(&e))
                                            >
                                                <For each=move || addresses.get() key=|a| a.id.clone() let:a>
                                                    <option value=a.id.clone() selected=move || address_id.get() == a.id>
                                                        {format!("{} — {}", a.label, a.resumo)}
                                                    </option>
                                                </For>
                                            </select>
                                        })}
                                        {move || (!novo_aberto.get()).then(|| view! {
                                            <button class="link-btn" on:click=move |_| set_novo_aberto.set(true)>"+ Novo endereço"</button>
                                        })}
                                        {move || novo_aberto.get().then(|| view! {
                                            <div class="cart-address-form">
                                                <input class="field" placeholder="Rótulo (ex.: Casa, Trabalho)" aria-label="Rótulo do endereço"
                                                    prop:value=move || a_label.get()
                                                    on:input=move |e| set_a_label.set(event_target_value(&e)) />
                                                <input class="field" placeholder="Rua" aria-label="Rua"
                                                    prop:value=move || a_rua.get()
                                                    on:input=move |e| set_a_rua.set(event_target_value(&e)) />
                                                <div class="cart-addr-grid">
                                                    <input class="field" placeholder="Número" aria-label="Número" inputmode="numeric"
                                                        prop:value=move || a_numero.get()
                                                        on:input=move |e| set_a_numero.set(event_target_value(&e)) />
                                                    <input class="field" placeholder="Bairro" aria-label="Bairro"
                                                        prop:value=move || a_bairro.get()
                                                        on:input=move |e| set_a_bairro.set(event_target_value(&e)) />
                                                </div>
                                                <input class="field" placeholder="Complemento (opcional)" aria-label="Complemento"
                                                    prop:value=move || a_compl.get()
                                                    on:input=move |e| set_a_compl.set(event_target_value(&e)) />
                                                <button class="cart-save-addr" on:click=salvar_endereco>"Salvar endereço"</button>
                                            </div>
                                        })}
                                    </div>
                                </div>
                            })}

                            // ── Observações ──
                            <div class="cart-sec">
                                <label class="cart-sec-title">"Observações"</label>
                                <textarea
                                    class="field"
                                    aria-label="Observações do pedido"
                                    placeholder="Alguma observação para a loja? (opcional)"
                                    prop:value=move || notes.get()
                                    on:input=move |e| set_notes.set(event_target_value(&e))
                                ></textarea>
                            </div>

                            // ── Cupom ──
                            <div class="cart-sec">
                                <label class="cart-sec-title">"Cupom de desconto"</label>
                                <div class="cart-promo">
                                    <input
                                        class="field"
                                        aria-label="Cupom de desconto"
                                        placeholder="Digite seu cupom"
                                        prop:value=move || coupon.get()
                                        on:input=move |e| set_coupon.set(event_target_value(&e))
                                    />
                                    <span class="cart-promo-apply">"Aplicar"</span>
                                </div>
                                {move || (!coupon.get().trim().is_empty()).then(|| view! {
                                    <p class="cart-hint">"O desconto do cupom é aplicado na confirmação do pedido."</p>
                                })}
                            </div>

                            <div class="cart-summary">
                                <div class="cart-total-row">
                                    <span>"Subtotal"</span>
                                    <span>{move || format::money(cart.total())}</span>
                                </div>
                                <div class="cart-total-row">
                                    <span>"Entrega"</span>
                                    {move || if delivery.get() && fee > 0.0 {
                                        view! { <span>{format::money(fee)}</span> }.into_any()
                                    } else {
                                        view! { <span class="cart-free">"Grátis"</span> }.into_any()
                                    }}
                                </div>
                                <div class="cart-total-row cart-total-final">
                                    <span>"Total"</span>
                                    <strong>{move || format::money(cart.total() + if delivery.get() { fee } else { 0.0 })}</strong>
                                </div>
                            </div>
                            {move || (!error.get().is_empty())
                                .then(|| view! { <p class="auth-error">{error.get()}</p> })}
                            <button class="cart-checkout" disabled=move || submitting.get() on:click=on_checkout>
                                {move || if !session.is_logged() {
                                    "Entrar para finalizar".to_string()
                                } else if submitting.get() {
                                    "Enviando…".to_string()
                                } else {
                                    "Finalizar pedido".to_string()
                                }}
                            </button>
                            </footer>
                            }.into_any()
                        }}
                    }.into_any(),
                }}
            </main>
        </div>
    }
}
