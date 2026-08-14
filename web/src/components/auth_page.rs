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
/// Código de recuperação: só dígitos, no máximo 6.
fn mask_code(s: &str) -> String {
    digits(s).chars().take(6).collect()
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
/// terminar/começar em `.`). A autoridade final é o backend (§11) — só UX.
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

/// Etapa da tela: login, cadastro ou os dois passos da recuperação de senha.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Login,
    Register,
    ForgotRequest,
    ForgotReset,
}

/// Página de login/cadastro/recuperação em rota própria (`/entrar`), no estilo
/// do cardápio (tema + cor de marca do tenant). Frontend burro: só coleta,
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
    let nome_header = info.name.clone();
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
    let (stage, set_stage) = signal(Stage::Login);
    let (ident, set_ident) = signal(String::new()); // e-mail/telefone (login) ou e-mail
    let (nome, set_nome) = signal(String::new());
    let (phone, set_phone) = signal(String::new());
    let (password, set_password) = signal(String::new());
    let (code, set_code) = signal(String::new());
    let (remember, set_remember) = signal(true);
    let (show_pw, set_show_pw) = signal(false);
    let (error, set_error) = signal(String::new());
    let (info_msg, set_info) = signal(String::new());
    let (busy, set_busy) = signal(false);

    // Validação por estágio (só apresentação; o backend revalida, §11).
    let validate = move |st: Stage| -> Result<(), String> {
        match st {
            Stage::Register => {
                if mask_nome(nome.get_untracked().trim()).trim().chars().count() < 2 {
                    return Err("Informe seu nome.".into());
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
            }
            Stage::Login => {
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
            Stage::ForgotRequest => {
                if !is_email(&ident.get_untracked()) {
                    return Err("Informe um e-mail válido.".into());
                }
            }
            Stage::ForgotReset => {
                if digits(&code.get_untracked()).len() != 6 {
                    return Err("Informe o código de 6 dígitos.".into());
                }
                if password.get_untracked().len() < 8 {
                    return Err("A senha deve ter ao menos 8 caracteres.".into());
                }
            }
        }
        Ok(())
    };

    let submit = move || {
        if busy.get_untracked() {
            return;
        }
        let st = stage.get_untracked();
        if let Err(msg) = validate(st) {
            set_error.set(msg);
            return;
        }
        set_error.set(String::new());
        set_busy.set(true);
        let id = ident.get_untracked();
        let n = nome.get_untracked();
        let p = digits(&phone.get_untracked()); // telefone só com dígitos p/ o backend
        let pw = password.get_untracked();
        let cd = digits(&code.get_untracked());
        let rem = remember.get_untracked();
        let navigate = use_navigate();
        spawn_local(async move {
            match st {
                Stage::Login => match session::customer_login(id, pw, rem).await {
                    Ok(info) => { session.set_remembering(info, rem); navigate("/", Default::default()); }
                    Err(e) => { set_error.set(format::server_error(&e.to_string())); set_busy.set(false); }
                },
                Stage::Register => match session::customer_register(n, id, p, pw).await {
                    Ok(info) => { session.set(info); navigate("/", Default::default()); }
                    Err(e) => { set_error.set(format::server_error(&e.to_string())); set_busy.set(false); }
                },
                Stage::ForgotRequest => match session::customer_forgot_password(id).await {
                    Ok(()) => {
                        set_busy.set(false);
                        set_info.set("Se o e-mail estiver cadastrado, enviamos um código de 6 dígitos.".into());
                        set_stage.set(Stage::ForgotReset);
                    }
                    Err(e) => { set_error.set(format::server_error(&e.to_string())); set_busy.set(false); }
                },
                Stage::ForgotReset => match session::customer_reset_password(id, cd, pw).await {
                    Ok(()) => {
                        set_busy.set(false);
                        set_password.set(String::new());
                        set_code.set(String::new());
                        set_info.set("Senha redefinida! Entre com a nova senha.".into());
                        set_stage.set(Stage::Login);
                    }
                    Err(e) => { set_error.set(format::server_error(&e.to_string())); set_busy.set(false); }
                },
            }
        });
    };

    // Troca de estágio limpando avisos.
    let goto = move |st: Stage| {
        set_error.set(String::new());
        set_info.set(String::new());
        set_stage.set(st);
    };

    view! {
        <div
            class="store-root auth-page"
            data-theme=theme
            style=move || if scheme.0.get() == "dark" { style_dark.clone() } else { style_light.clone() }
        >
            <Title text=move || match stage.get() {
                Stage::Register => "Criar conta".to_string(),
                Stage::ForgotRequest | Stage::ForgotReset => "Recuperar senha".to_string(),
                Stage::Login => format!("Entrar — {nome_loja}"),
            }/>
            <main class="auth-card" role="main">
                // Marca da loja: a LOGO cadastrada, se houver; senão um badge
                // com ícone + o nome da empresa, na cor cadastrada no painel.
                {match logo {
                    Some(l) => view! {
                        <a class="auth-brand" href="/" aria-label="Voltar ao cardápio">
                            <img class="auth-logo" src=l alt="" />
                        </a>
                    }.into_any(),
                    None => view! {
                        <a class="auth-brand auth-brand--name" href="/" aria-label="Voltar ao cardápio">
                            <span class="auth-badge" aria-hidden="true"><Icon name="empresa"/></span>
                            <span class="auth-brand-name">{nome_header.clone()}</span>
                        </a>
                    }.into_any(),
                }}

                <h1 class="auth-title">
                    {move || match stage.get() {
                        Stage::Login => "Entrar",
                        Stage::Register => "Criar conta",
                        Stage::ForgotRequest => "Recuperar senha",
                        Stage::ForgotReset => "Nova senha",
                    }}
                </h1>
                <p class="auth-sub">
                    {move || match stage.get() {
                        Stage::Login => "Acesse sua conta para pedir mais rápido",
                        Stage::Register => "Crie sua conta para pedir mais rápido",
                        Stage::ForgotRequest => "Enviaremos um código para o seu e-mail",
                        Stage::ForgotReset => "Digite o código enviado e a nova senha",
                    }}
                </p>

                <form class="auth-body" novalidate=true
                    on:submit=move |ev: leptos::ev::SubmitEvent| { ev.prevent_default(); submit(); }>

                    // Nome (só cadastro)
                    {move || (stage.get() == Stage::Register).then(|| view! {
                        <div class="field-wrap">
                            <span class="field-lead" aria-hidden="true"><Icon name="usuario"/></span>
                            <input class="field has-lead" r#type="text" placeholder="Nome" aria-label="Nome"
                                autocomplete="name"
                                prop:value=move || nome.get()
                                on:input=move |e| set_nome.set(mask_nome(&event_target_value(&e))) />
                        </div>
                    })}

                    // E-mail / telefone (login, cadastro e pedir código)
                    {move || (stage.get() != Stage::ForgotReset).then(|| view! {
                        <div class="field-wrap">
                            <span class="field-lead" aria-hidden="true"><Icon name="email"/></span>
                            <input
                                class="field has-lead"
                                r#type="text"
                                inputmode=move || if stage.get() == Stage::Login { "text" } else { "email" }
                                placeholder=move || if stage.get() == Stage::Login { "E-mail ou telefone" } else { "E-mail" }
                                aria-label=move || if stage.get() == Stage::Login { "E-mail ou telefone" } else { "E-mail" }
                                autocomplete="username"
                                prop:value=move || ident.get()
                                on:input=move |e| {
                                    let v = event_target_value(&e);
                                    set_ident.set(if stage.get_untracked() == Stage::Login { mask_ident(&v) } else { mask_email(&v) });
                                } />
                        </div>
                    })}

                    // Telefone (só cadastro)
                    {move || (stage.get() == Stage::Register).then(|| view! {
                        <div class="field-wrap">
                            <span class="field-lead" aria-hidden="true"><Icon name="telefone"/></span>
                            <input class="field has-lead" r#type="text" inputmode="numeric"
                                placeholder="Telefone" aria-label="Telefone" autocomplete="tel"
                                prop:value=move || phone.get()
                                on:input=move |e| set_phone.set(mask_phone(&event_target_value(&e))) />
                        </div>
                    })}

                    // Código de recuperação (só ForgotReset)
                    {move || (stage.get() == Stage::ForgotReset).then(|| view! {
                        <div class="field-wrap">
                            <span class="field-lead" aria-hidden="true"><Icon name="cadeado-aberto"/></span>
                            <input class="field has-lead" r#type="text" inputmode="numeric"
                                placeholder="Código de 6 dígitos" aria-label="Código de recuperação"
                                prop:value=move || code.get()
                                on:input=move |e| set_code.set(mask_code(&event_target_value(&e))) />
                        </div>
                    })}

                    // Senha (login, cadastro e nova senha)
                    {move || (stage.get() != Stage::ForgotRequest).then(|| view! {
                        <div class="field-wrap">
                            <span class="field-lead" aria-hidden="true"><Icon name="cadeado-aberto"/></span>
                            <input
                                class="field has-lead has-eye"
                                r#type=move || if show_pw.get() { "text" } else { "password" }
                                placeholder=move || if stage.get() == Stage::ForgotReset { "Nova senha" } else { "Senha" }
                                aria-label=move || if stage.get() == Stage::ForgotReset { "Nova senha" } else { "Senha" }
                                autocomplete=move || if stage.get() == Stage::Login { "current-password" } else { "new-password" }
                                prop:value=move || password.get()
                                on:input=move |e| set_password.set(event_target_value(&e)) />
                            <button type="button" class="pw-eye"
                                on:click=move |_| set_show_pw.update(|v| *v = !*v)
                                aria-label=move || if show_pw.get() { "Ocultar senha" } else { "Mostrar senha" }>
                                {move || if show_pw.get() {
                                    view! { <Icon name="ocultar"/> }.into_any()
                                } else {
                                    view! { <Icon name="visualizar"/> }.into_any()
                                }}
                            </button>
                        </div>
                    })}

                    // Linha: Lembrar + Esqueceu a senha (só login)
                    {move || (stage.get() == Stage::Login).then(|| view! {
                        <div class="auth-row">
                            <label class="auth-remember">
                                <input type="checkbox"
                                    prop:checked=move || remember.get()
                                    on:change=move |e| set_remember.set(event_target_checked(&e)) />
                                <span class="auth-remember-box" aria-hidden="true"><Icon name="confirmar"/></span>
                                <span>"Lembrar"</span>
                            </label>
                            <button type="button" class="auth-foot-link" on:click=move |_| goto(Stage::ForgotRequest)>
                                "Esqueceu a senha?"
                            </button>
                        </div>
                    })}

                    <button type="submit" class="auth-submit" disabled=move || busy.get()>
                        {move || if busy.get() {
                            "Aguarde…"
                        } else {
                            match stage.get() {
                                Stage::Login => "Entrar",
                                Stage::Register => "Cadastrar",
                                Stage::ForgotRequest => "Enviar código",
                                Stage::ForgotReset => "Redefinir senha",
                            }
                        }}
                    </button>
                </form>

                <p class="auth-foot">
                    {move || match stage.get() {
                        Stage::Login => view! {
                            "Não tem conta? "
                            <button type="button" class="auth-foot-link" on:click=move |_| goto(Stage::Register)>"Cadastre-se"</button>
                        }.into_any(),
                        Stage::Register => view! {
                            "Já tem conta? "
                            <button type="button" class="auth-foot-link" on:click=move |_| goto(Stage::Login)>"Entrar"</button>
                        }.into_any(),
                        _ => view! {
                            <button type="button" class="auth-foot-link" on:click=move |_| goto(Stage::Login)>"Voltar ao login"</button>
                        }.into_any(),
                    }}
                </p>
            </main>

            // Avisos ABAIXO do card (sem alertas/modais nativos): erro em
            // vermelho, informação em verde.
            {move || (!error.get().is_empty()).then(|| view! {
                <p class="auth-page-error" role="alert" aria-live="polite">{error.get()}</p>
            })}
            {move || (!info_msg.get().is_empty()).then(|| view! {
                <p class="auth-page-info" role="status" aria-live="polite">{info_msg.get()}</p>
            })}
        </div>
    }
}
