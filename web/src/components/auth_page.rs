use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_meta::Title;
use leptos_router::hooks::use_navigate;

use crate::api::CatalogInfo;
use crate::components::icon::Icon;
use crate::format;
use crate::session::{self, Session};
use crate::theme::Scheme;

/// Server function: lê o `Host` da requisição, resolve o tenant e busca só a
/// info pública (nome/logo/tema/cor de marca) — a tela de login herda a
/// identidade da loja sem carregar o catálogo inteiro (§1/§11, frontend burro).
#[server]
pub async fn get_catalog_info() -> Result<CatalogInfo, ServerFnError> {
    use axum::http::{header::HOST, HeaderMap};
    let headers: HeaderMap = leptos_axum::extract().await?;
    let host = headers
        .get(HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    crate::api::fetch_catalog_info(&host)
        .await
        .map_err(ServerFnError::new)
}

/// Página de login/cadastro em rota própria (`/entrar`), no estilo do
/// cardápio (tema + cor de marca do tenant). Frontend burro: só coleta e
/// chama a server fn, que faz proxy à API — quem valida e emite o JWT é o
/// backend (§11).
#[component]
pub fn AuthPage() -> impl IntoView {
    let info = Resource::new_blocking(|| (), |_| get_catalog_info());

    view! {
        <Suspense fallback=|| view! { <p class="state">"Carregando…"</p> }>
            {move || Suspend::new(async move {
                match info.await {
                    Ok(info) => view! { <AuthView info/> }.into_any(),
                    // Sem a marca ainda dá para logar — renderiza o formulário
                    // com o tema padrão.
                    Err(_) => view! { <AuthView info=CatalogInfo::default()/> }.into_any(),
                }
            })}
        </Suspense>
    }
}

#[component]
fn AuthView(info: CatalogInfo) -> impl IntoView {
    let session = expect_context::<Session>();
    let scheme = expect_context::<Scheme>();

    // Identidade da loja para o topo do card.
    let nome = info.name.clone();
    let logo = info.logo_url.clone();
    let theme = info.theme.clone();
    let default_scheme = info.default_scheme.clone();

    // Cor de marca (hex validado) → sobrepõe `--brand` e deriva `--brand-ink`
    // por esquema. Mesmas regras do catálogo (sem injeção de CSS, §11).
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

    // Estado inicial do esquema: escolha salva do visitante PREVALECE; senão
    // o padrão da empresa; senão o sistema (igual ao catálogo).
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

    // ── Estado do formulário ────────────────────────────────────────────
    let (is_register, set_is_register) = signal(false);
    let (ident, set_ident) = signal(String::new()); // e-mail/telefone (login) ou e-mail (cadastro)
    let (name, set_name) = signal(String::new());
    let (phone, set_phone) = signal(String::new());
    let (password, set_password) = signal(String::new());
    let (remember, set_remember) = signal(true);
    let (show_pw, set_show_pw) = signal(false);
    let (error, set_error) = signal(String::new());
    let (busy, set_busy) = signal(false);

    let submit = move || {
        if busy.get_untracked() {
            return;
        }
        set_error.set(String::new());
        set_busy.set(true);
        let reg = is_register.get_untracked();
        let id = ident.get_untracked();
        let n = name.get_untracked();
        let p = phone.get_untracked();
        let pw = password.get_untracked();
        let rem = remember.get_untracked();
        let navigate = use_navigate();
        spawn_local(async move {
            let res = if reg {
                session::customer_register(n, id, p, pw).await
            } else {
                session::customer_login(id, pw, rem).await
            };
            match res {
                Ok(info) => {
                    if reg {
                        session.set(info);
                    } else {
                        session.set_remembering(info, rem);
                    }
                    navigate("/", Default::default());
                }
                Err(err) => {
                    set_error.set(format::server_error(&err.to_string()));
                    set_busy.set(false);
                }
            }
        });
    };

    view! {
        <div
            class="store-root auth-page"
            data-theme=theme
            style=move || if scheme.0.get() == "dark" { style_dark.clone() } else { style_light.clone() }
        >
            <Title text=move || if is_register.get() { "Criar conta".to_string() } else { format!("Entrar — {nome}") }/>
            <main class="auth-card" role="main">
                <a class="auth-brand" href="/" aria-label="Voltar ao cardápio">
                    {logo.map(|l| view! { <img class="auth-logo" src=l alt="" /> })}
                    <span class="auth-brand-name">{info.name.clone()}</span>
                </a>

                <h1 class="auth-title">
                    {move || if is_register.get() { "Criar conta" } else { "Entrar" }}
                </h1>
                <p class="auth-sub">
                    {move || if is_register.get() {
                        "Crie sua conta para pedir mais rápido"
                    } else {
                        "Acesse sua conta para pedir mais rápido"
                    }}
                </p>

                <form class="auth-body"
                    on:submit=move |ev: leptos::ev::SubmitEvent| { ev.prevent_default(); submit(); }>

                    {move || is_register.get().then(|| view! {
                        <div class="field-wrap">
                            <input
                                class="field"
                                placeholder="Nome"
                                aria-label="Nome"
                                prop:value=move || name.get()
                                on:input=move |e| set_name.set(event_target_value(&e))
                            />
                        </div>
                    })}

                    <div class="field-wrap">
                        <span class="field-lead" aria-hidden="true"><Icon name="email"/></span>
                        <input
                            class="field has-lead"
                            r#type=move || if is_register.get() { "email" } else { "text" }
                            placeholder=move || if is_register.get() { "E-mail" } else { "E-mail ou telefone" }
                            aria-label=move || if is_register.get() { "E-mail" } else { "E-mail ou telefone" }
                            autocomplete="username"
                            prop:value=move || ident.get()
                            on:input=move |e| set_ident.set(event_target_value(&e))
                        />
                    </div>

                    {move || is_register.get().then(|| view! {
                        <div class="field-wrap">
                            <input
                                class="field"
                                placeholder="Telefone (opcional)"
                                aria-label="Telefone (opcional)"
                                prop:value=move || phone.get()
                                on:input=move |e| set_phone.set(event_target_value(&e))
                            />
                        </div>
                    })}

                    <div class="field-wrap">
                        <input
                            class="field has-eye"
                            r#type=move || if show_pw.get() { "text" } else { "password" }
                            placeholder="Senha"
                            aria-label="Senha"
                            autocomplete=move || if is_register.get() { "new-password" } else { "current-password" }
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

                    {move || (!is_register.get()).then(|| view! {
                        <label class="auth-remember">
                            <input
                                type="checkbox"
                                prop:checked=move || remember.get()
                                on:change=move |e| set_remember.set(event_target_checked(&e))
                            />
                            <span>"Lembrar de mim"</span>
                        </label>
                    })}

                    {move || (!error.get().is_empty())
                        .then(|| view! { <p class="auth-error" role="alert">{error.get()}</p> })}

                    <button type="submit" class="pm-add auth-submit" disabled=move || busy.get()>
                        {move || if busy.get() {
                            "Aguarde…".to_string()
                        } else if is_register.get() {
                            "Cadastrar".to_string()
                        } else {
                            "Entrar".to_string()
                        }}
                    </button>
                </form>

                <p class="auth-foot">
                    {move || if is_register.get() { "Já tem conta? " } else { "Não tem conta? " }}
                    <button
                        type="button"
                        class="auth-foot-link"
                        on:click=move |_| {
                            set_error.set(String::new());
                            set_is_register.update(|v| *v = !*v);
                        }
                    >
                        {move || if is_register.get() { "Entrar" } else { "Cadastre-se" }}
                    </button>
                </p>
            </main>
        </div>
    }
}
