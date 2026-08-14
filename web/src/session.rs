//! Sessão do cliente final: identidade de EXIBIÇÃO (nome/id) no cliente; o
//! JWT vive num cookie HttpOnly setado pelo SSR — inacessível a JS/XSS
//! (§11). As `#[server]` fns fazem proxy para a API (forward de `Host`),
//! igual ao catálogo — o navegador nunca fala direto com a API, e nunca
//! recebe o token: login/registro gravam o cookie e devolvem só nome/id; as
//! ações autenticadas leem o token do cookie no servidor (`require_token`).

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

/// Identidade da sessão (resposta de `/customer/{login,register}`).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SessionInfo {
    pub token: String,
    pub customer_id: String,
    pub name: String,
}

/// Contexto da sessão (`None` = deslogado).
#[derive(Clone, Copy)]
pub struct Session(pub RwSignal<Option<SessionInfo>>);

impl Session {
    pub fn set(&self, info: SessionInfo) {
        self.0.set(Some(info));
        self.persist();
    }

    /// Guarda a sessão escolhendo onde persistir a exibição: `remember=true`
    /// → localStorage (persistente); `false` → sessionStorage (só a sessão do
    /// navegador). Espelha o cookie gravado pela server fn de login.
    pub fn set_remembering(&self, info: SessionInfo, remember: bool) {
        self.0.set(Some(info));
        save_where(&self.0.get_untracked(), remember);
    }

    pub fn clear(&self) {
        self.0.set(None);
        self.persist();
    }

    /// Atualiza o nome exibido (após editar o perfil), mantendo o token.
    pub fn rename(&self, name: String) {
        self.0.update(|s| {
            if let Some(i) = s {
                i.name = name;
            }
        });
        self.persist();
    }

    /// Nome do cliente logado (reativo).
    pub fn name(&self) -> Option<String> {
        self.0.with(|s| s.as_ref().map(|i| i.name.clone()))
    }

    /// Está logado? (reativo)
    pub fn is_logged(&self) -> bool {
        self.0.with(|s| s.is_some())
    }

    fn persist(&self) {
        save(&self.0.get_untracked());
    }
}

#[cfg(feature = "hydrate")]
const KEY: &str = "letaf:session";

#[cfg(feature = "hydrate")]
fn read_storage(local: bool) -> Option<web_sys::Storage> {
    let w = web_sys::window()?;
    if local { w.local_storage().ok().flatten() } else { w.session_storage().ok().flatten() }
}

#[cfg(feature = "hydrate")]
pub fn load() -> Option<SessionInfo> {
    // Persistente (localStorage) tem prioridade; senão a sessão do navegador
    // (sessionStorage, login sem "Lembrar de mim").
    [true, false]
        .into_iter()
        .filter_map(|local| read_storage(local)?.get_item(KEY).ok().flatten())
        .find_map(|json| serde_json::from_str::<SessionInfo>(&json).ok())
}

#[cfg(not(feature = "hydrate"))]
pub fn load() -> Option<SessionInfo> {
    None
}

/// Persiste a exibição da sessão. `persistent=true` → localStorage (sobrevive
/// a fechar o navegador, "Lembrar de mim"); `false` → sessionStorage (só a
/// sessão atual). Sempre limpa o OUTRO storage para não ficar sessão fantasma.
#[cfg(feature = "hydrate")]
fn save_where(info: &Option<SessionInfo>, persistent: bool) {
    let (this, other) = (read_storage(persistent), read_storage(!persistent));
    if let Some(s) = other {
        let _ = s.remove_item(KEY);
    }
    if let Some(s) = this {
        match info {
            Some(i) => {
                if let Ok(json) = serde_json::to_string(i) {
                    let _ = s.set_item(KEY, &json);
                }
            }
            None => {
                let _ = s.remove_item(KEY);
            }
        }
    }
}

#[cfg(feature = "hydrate")]
fn save(info: &Option<SessionInfo>) {
    save_where(info, true);
}

#[cfg(not(feature = "hydrate"))]
fn save(_info: &Option<SessionInfo>) {}

#[cfg(not(feature = "hydrate"))]
fn save_where(_info: &Option<SessionInfo>, _persistent: bool) {}

/// Lê o `Host` da requisição SSR (resolve o tenant na API). Reusado
/// pelas server fns de auth/conta.
#[cfg(feature = "ssr")]
pub(crate) async fn tenant_host() -> Result<String, ServerFnError> {
    use axum::http::{header::HOST, HeaderMap};
    let headers: HeaderMap = leptos_axum::extract().await?;
    Ok(headers
        .get(HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string())
}

/// Nome do cookie HttpOnly que guarda o JWT do cliente.
#[cfg(feature = "ssr")]
const COOKIE_NAME: &str = "letaf_session";

/// Lê o JWT do cookie da requisição (server-side). O cliente nunca vê o token.
#[cfg(feature = "ssr")]
pub(crate) async fn read_token_cookie() -> Option<String> {
    use axum::http::{header::COOKIE, HeaderMap};
    let headers: HeaderMap = leptos_axum::extract().await.ok()?;
    let raw = headers.get(COOKIE)?.to_str().ok()?;
    let prefix = format!("{COOKIE_NAME}=");
    raw.split(';')
        .map(str::trim)
        .find_map(|p| p.strip_prefix(&prefix))
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// Exige um token de sessão (cookie) ou falha com erro amigável — usado pelas
/// ações autenticadas no lugar de receber o token do cliente (§11). Também
/// exige MESMA ORIGEM (anti-CSRF) antes de liberar a ação.
#[cfg(feature = "ssr")]
pub(crate) async fn require_token() -> Result<String, ServerFnError> {
    verify_same_origin().await?;
    read_token_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("Sessão expirada. Faça login novamente."))
}

/// Host é de DESENVOLVIMENTO local? Ancorado — não casa "localhost.evil.com"
/// nem "x.localhost.evil.com" (o `starts_with`/`contains` antigo casava). Tira
/// a porta antes de comparar.
#[cfg(feature = "ssr")]
pub(crate) fn host_is_local(host: &str) -> bool {
    let h = host.split(':').next().unwrap_or(host);
    h == "localhost" || h.ends_with(".localhost") || h == "127.0.0.1"
}

/// Defesa CSRF (§11): compara o host de `Origin`/`Referer` com o `Host` do
/// request. Um POST cross-site (o vetor de CSRF) SEMPRE carrega uma Origin
/// diferente → barrado — fechando a janela "Lax+POST" que o SameSite=Lax deixa
/// aberta por ~2 min após o login. Ausência de Origin/Referer (navegação direta
/// same-origin) não bloqueia, para não gerar falso-positivo.
#[cfg(feature = "ssr")]
async fn verify_same_origin() -> Result<(), ServerFnError> {
    use axum::http::header::{HOST, ORIGIN, REFERER};
    use axum::http::HeaderMap;
    let headers: HeaderMap = leptos_axum::extract().await?;
    let host = headers.get(HOST).and_then(|v| v.to_str().ok()).unwrap_or_default();
    let claimed = headers
        .get(ORIGIN)
        .and_then(|v| v.to_str().ok())
        .or_else(|| headers.get(REFERER).and_then(|v| v.to_str().ok()));
    if let Some(url) = claimed {
        // Extrai o host de "scheme://host[:porta]/...".
        let h = url.split("://").nth(1).unwrap_or("").split('/').next().unwrap_or("");
        if !h.is_empty() && !h.eq_ignore_ascii_case(host) {
            return Err(ServerFnError::new("Origem inválida (possível CSRF)."));
        }
    }
    Ok(())
}

/// `Secure` só em contexto HTTPS (produção); em dev http (localhost) o cookie
/// Secure não seria aceito e travaria o login. Espelha o esquema do catálogo.
#[cfg(feature = "ssr")]
async fn secure_context() -> bool {
    use axum::http::{header::HOST, HeaderMap};
    let headers: HeaderMap = match leptos_axum::extract().await {
        Ok(h) => h,
        Err(_) => return true, // na dúvida, exige HTTPS (mais restritivo)
    };
    if let Some(p) = headers.get("x-forwarded-proto").and_then(|v| v.to_str().ok()) {
        return p.eq_ignore_ascii_case("https");
    }
    let host = headers.get(HOST).and_then(|v| v.to_str().ok()).unwrap_or_default();
    !host_is_local(host)
}

/// Sentinela para `write_cookie`: cookie de SESSÃO (sem `Max-Age` → o
/// navegador o descarta ao fechar). Usado no login sem "Lembrar de mim".
#[cfg(feature = "ssr")]
const SESSION_COOKIE: i64 = -1;

/// Grava o cookie de sessão. `HttpOnly` (JS não lê → sem exfiltração por XSS),
/// `SameSite=Lax` (não é enviado em POST cross-site → CSRF nas server-fns),
/// `Path=/`. `max_age>0` → cookie persistente com esse `Max-Age` (alinhado ao
/// exp do JWT, 72h); `max_age=0` limpa; `max_age<0` → cookie de SESSÃO (sem
/// `Max-Age`).
#[cfg(feature = "ssr")]
async fn write_cookie(value: &str, max_age: i64) {
    use axum::http::header::SET_COOKIE;
    use axum::http::HeaderValue;
    let secure = if secure_context().await { "; Secure" } else { "" };
    let age = if max_age < 0 {
        String::new()
    } else {
        format!("; Max-Age={max_age}")
    };
    let cookie = format!("{COOKIE_NAME}={value}; HttpOnly; SameSite=Lax; Path=/{age}{secure}");
    if let Ok(hv) = HeaderValue::from_str(&cookie) {
        expect_context::<leptos_axum::ResponseOptions>().append_header(SET_COOKIE, hv);
    }
}

/// Remove o token da resposta ao cliente: ele fica SÓ no cookie HttpOnly.
#[cfg(feature = "ssr")]
fn display_only(info: SessionInfo) -> SessionInfo {
    SessionInfo { token: String::new(), ..info }
}

/// POST /customer/login (proxy). Backend valida; o JWT vai para um cookie
/// HttpOnly e NÃO é devolvido ao navegador (só nome/id para exibição).
#[server]
pub async fn customer_login(
    identifier: String,
    password: String,
    remember: bool,
) -> Result<SessionInfo, ServerFnError> {
    verify_same_origin().await?;
    let host = tenant_host().await?;
    // `identifier` = e-mail OU telefone; a API resolve/valida (§11).
    let info = crate::api::customer_login(&host, &identifier, &password)
        .await
        .map_err(ServerFnError::new)?;
    // "Lembrar de mim": cookie PERSISTENTE (72h, alinhado ao exp do JWT) quando
    // marcado; senão cookie de SESSÃO (some ao fechar o navegador).
    write_cookie(&info.token, if remember { 259_200 } else { SESSION_COOKIE }).await;
    Ok(display_only(info))
}

/// POST /customer/register (proxy). Idem: cookie HttpOnly, sem token no corpo.
#[server]
pub async fn customer_register(
    name: String,
    email: String,
    phone: String,
    password: String,
) -> Result<SessionInfo, ServerFnError> {
    verify_same_origin().await?;
    let host = tenant_host().await?;
    let info = crate::api::customer_register(&host, &name, &email, &phone, &password)
        .await
        .map_err(ServerFnError::new)?;
    write_cookie(&info.token, 259_200).await;
    Ok(display_only(info))
}

/// POST /customer/forgot-password (proxy). Sempre 200 (anti-enumeração).
#[server]
pub async fn customer_forgot_password(email: String) -> Result<(), ServerFnError> {
    verify_same_origin().await?;
    let host = tenant_host().await?;
    crate::api::customer_forgot_password(&host, &email)
        .await
        .map_err(ServerFnError::new)
}

/// POST /customer/verify-reset-code (proxy). Valida o código sem consumir.
#[server]
pub async fn customer_verify_reset(email: String, code: String) -> Result<(), ServerFnError> {
    verify_same_origin().await?;
    let host = tenant_host().await?;
    crate::api::customer_verify_reset(&host, &email, &code)
        .await
        .map_err(ServerFnError::new)
}

/// POST /customer/reset-password (proxy). Consome o código e troca a senha.
#[server]
pub async fn customer_reset_password(
    email: String,
    code: String,
    new_password: String,
) -> Result<(), ServerFnError> {
    verify_same_origin().await?;
    let host = tenant_host().await?;
    crate::api::customer_reset_password(&host, &email, &code, &new_password)
        .await
        .map_err(ServerFnError::new)
}

/// Encerra a sessão: apaga o cookie HttpOnly (o cliente já limpa a exibição).
#[server]
pub async fn customer_logout() -> Result<(), ServerFnError> {
    verify_same_origin().await?;
    write_cookie("", 0).await;
    Ok(())
}
