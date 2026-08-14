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

// ── Máscaras (só deixam passar os caracteres pertinentes ao campo) ──────────
/// Só letras e espaços (nome).
fn mask_nome(s: &str) -> String {
    s.chars().filter(|c| c.is_alphabetic() || *c == ' ').collect()
}
/// Caracteres válidos de e-mail.
fn mask_email(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric() || "@._-+".contains(*c))
        .collect()
}
/// E-mail OU telefone (login): letras/dígitos + pontuação de e-mail.
fn mask_ident(s: &str) -> String {
    mask_email(s)
}
/// Só dígitos.
fn digits(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_digit()).collect()
}
/// Telefone brasileiro com máscara `(99) 99999-9999` (até 11 dígitos).
fn mask_phone(s: &str) -> String {
    let d = digits(s);
    let d: String = d.chars().take(11).collect();
    let n = d.len();
    if n == 0 {
        return String::new();
    }
    let mut out = String::from("(");
    out.push_str(&d[..n.min(2)]);
    if n > 2 {
        out.push_str(") ");
        let rest = &d[2..];
        let split = if n > 10 { 5 } else { 4 };
        if rest.len() <= split {
            out.push_str(rest);
        } else {
            out.push_str(&rest[..split]);
            out.push('-');
            out.push_str(&rest[split..]);
        }
    }
    out
}

/// E-mail válido de forma simples (algo antes do `@`, um `.` depois, sem
/// terminar em `.`). A autoridade final é o backend (§11) — isto é só UX.
fn is_email(s: &str) -> bool {
    let s = s.trim();
    match s.find('@') {
        Some(i) if i > 0 => {
            let dom = &s[i + 1..];
            dom.contains('.') && !dom.ends_with('.') && !dom.starts_with('.')
        }
        _ => false,
    }
}
fn is_phone(s: &str) -> bool {
    let n = digits(s).len();
    (10..=11).contains(&n)
}

/// Página de login/cadastro em rota própria (`/entrar`), no estilo do
/// cardápio (tema + cor de marca do tenant). Frontend burro: só coleta,
/// valida a apresentação e chama a server fn — a autoridade é o backend (§11).
#[component]
pub fn AuthPage() -> impl IntoView {
    let info = Resource::new_blocking(|| (), |_| get_catalog_info());

    view! {
        <Suspense fallback=|| view! { <p class="state">"Carregando…"</p> }>
            {move || Suspend::new(async move {
                match info.await {
                    Ok(info) => view! { <AuthView info/> }.into_any(),
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

    let nome_loja = info.name.clone();
    let logo = info.logo_url.clone();
    let theme = info.theme.clone();
    let default_scheme = info.default_scheme.clone();

    // Cor de marca (hex validado) → sobrepõe `--brand`/`--brand-ink` por
    // esquema (sem injeção de CSS, §11). O botão usa a cor da empresa.
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

    // Esquema inicial: escolha salva PREVALECE; senão padrão da empresa; senão
    // sistema (igual ao catálogo).
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
    let (nome, set_nome) = signal(String::new());
    let (phone, set_phone) = signal(String::new());
    let (password, set_password) = signal(String::new());
    let (remember, set_remember) = signal(true);
    let (show_pw, set_show_pw) = signal(false);
    let (error, set_error) = signal(String::new());
    let (busy, set_busy) = signal(false);

    // Validação por campo (só apresentação; o backend revalida, §11).
    let validate = move |reg: bool| -> Result<(), String> {
        if reg {
            if mask_nome(nome.get_untracked().trim()).trim().chars().count() < 2 {
                return Err("Informe seu nome (só letras).".into());
            }
            if !is_email(&ident.get_untracked()) {
                return Err("Informe um e-mail válido.".into());
            }
            if !is_phone(&phone.get_untracked()) {
                return Err("Informe um telefone válido com DDD.".into());
            }
            if password.get_untracked().len() < 8 {
                return Err("A senha deve ter ao menos 8 caracteres.".into());
            }
        } else {
            let id = ident.get_untracked();
            let id = id.trim();
            if id.is_empty() {
                return Err("Informe seu e-mail ou telefone.".into());
            }
            let ok = if id.contains('@') { is_email(id) } else { is_phone(id) };
            if !ok {
                return Err("Informe um e-mail válido ou um telefone com DDD.".into());
            }
            if password.get_untracked().is_empty() {
                return Err("Informe sua senha.".into());
            }
        }
        Ok(())
    };

    let submit = move || {
        if busy.get_untracked() {
            return;
        }
        let reg = is_register.get_untracked();
        if let Err(msg) = validate(reg) {
            set_error.set(msg);
            return;
        }
        set_error.set(String::new());
        set_busy.set(true);
        let id = ident.get_untracked();
        let n = nome.get_untracked();
        let p = digits(&phone.get_untracked()); // telefone só com dígitos p/ o backend
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
            <Title text=move || if is_register.get() { "Criar conta".to_string() } else { format!("Entrar — {nome_loja}") }/>
            <main class="auth-card" role="main">
                // Logo do estabelecimento; sem logo → nada (nem nome).
                {logo.map(|l| view! {
                    <a class="auth-brand" href="/" aria-label="Voltar ao cardápio">
                        <img class="auth-logo" src=l alt="" />
                    </a>
                })}

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

                <form class="auth-body" novalidate=true
                    on:submit=move |ev: leptos::ev::SubmitEvent| { ev.prevent_default(); submit(); }>

                    {move || is_register.get().then(|| view! {
                        <div class="field-wrap">
                            <span class="field-lead" aria-hidden="true"><Icon name="usuario"/></span>
                            <input
                                class="field has-lead"
                                r#type="text"
                                placeholder="Nome"
                                aria-label="Nome"
                                autocomplete="name"
                                prop:value=move || nome.get()
                                on:input=move |e| set_nome.set(mask_nome(&event_target_value(&e)))
                            />
                        </div>
                    })}

                    <div class="field-wrap">
                        <span class="field-lead" aria-hidden="true"><Icon name="email"/></span>
                        <input
                            class="field has-lead"
                            r#type="text"
                            inputmode=move || if is_register.get() { "email" } else { "text" }
                            placeholder=move || if is_register.get() { "E-mail" } else { "E-mail ou telefone" }
                            aria-label=move || if is_register.get() { "E-mail" } else { "E-mail ou telefone" }
                            autocomplete="username"
                            prop:value=move || ident.get()
                            on:input=move |e| {
                                let v = event_target_value(&e);
                                set_ident.set(if is_register.get_untracked() { mask_email(&v) } else { mask_ident(&v) });
                            }
                        />
                    </div>

                    {move || is_register.get().then(|| view! {
                        <div class="field-wrap">
                            <span class="field-lead" aria-hidden="true"><Icon name="telefone"/></span>
                            <input
                                class="field has-lead"
                                r#type="text"
                                inputmode="numeric"
                                placeholder="Telefone"
                                aria-label="Telefone"
                                autocomplete="tel"
                                prop:value=move || phone.get()
                                on:input=move |e| set_phone.set(mask_phone(&event_target_value(&e)))
                            />
                        </div>
                    })}

                    <div class="field-wrap">
                        <span class="field-lead" aria-hidden="true"><Icon name="cadeado"/></span>
                        <input
                            class="field has-lead has-eye"
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
                            <span class="auth-remember-box" aria-hidden="true"></span>
                            <span>"Lembrar de mim"</span>
                        </label>
                    })}

                    <button type="submit" class="auth-submit" disabled=move || busy.get()>
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

            // Mensagem de erro ABAIXO do card, em vermelho (sem alertas/modais
            // nativos do navegador).
            {move || (!error.get().is_empty()).then(|| view! {
                <p class="auth-page-error" role="alert" aria-live="polite">{error.get()}</p>
            })}
        </div>
    }
}
