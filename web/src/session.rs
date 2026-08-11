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
pub fn load() -> Option<SessionInfo> {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item(KEY).ok().flatten())
        .and_then(|json| serde_json::from_str::<SessionInfo>(&json).ok())
}

#[cfg(not(feature = "hydrate"))]
pub fn load() -> Option<SessionInfo> {
    None
}

#[cfg(feature = "hydrate")]
fn save(info: &Option<SessionInfo>) {
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        match info {
            Some(i) => {
                if let Ok(json) = serde_json::to_string(i) {
                    let _ = storage.set_item(KEY, &json);
                }
            }
            None => {
                let _ = storage.remove_item(KEY);
            }
        }
    }
}

#[cfg(not(feature = "hydrate"))]
fn save(_info: &Option<SessionInfo>) {}

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
/// ações autenticadas no lugar de receber o token do cliente (§11).
#[cfg(feature = "ssr")]
pub(crate) async fn require_token() -> Result<String, ServerFnError> {
    read_token_cookie()
        .await
        .ok_or_else(|| ServerFnError::new("Sessão expirada. Faça login novamente."))
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
    !(host.starts_with("localhost") || host.contains(".localhost") || host.starts_with("127.0.0.1"))
}

/// Grava o cookie de sessão. `HttpOnly` (JS não lê → sem exfiltração por XSS),
/// `SameSite=Lax` (não é enviado em POST cross-site → CSRF nas server-fns),
/// `Path=/`, `Max-Age` alinhado ao exp do JWT (72h). `max_age=0` limpa.
#[cfg(feature = "ssr")]
async fn write_cookie(value: &str, max_age: i64) {
    use axum::http::header::SET_COOKIE;
    use axum::http::HeaderValue;
    let secure = if secure_context().await { "; Secure" } else { "" };
    let cookie = format!(
        "{COOKIE_NAME}={value}; HttpOnly; SameSite=Lax; Path=/; Max-Age={max_age}{secure}"
    );
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
pub async fn customer_login(email: String, password: String) -> Result<SessionInfo, ServerFnError> {
    let host = tenant_host().await?;
    let info = crate::api::customer_login(&host, &email, &password)
        .await
        .map_err(ServerFnError::new)?;
    write_cookie(&info.token, 259_200).await;
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
    let host = tenant_host().await?;
    let info = crate::api::customer_register(&host, &name, &email, &phone, &password)
        .await
        .map_err(ServerFnError::new)?;
    write_cookie(&info.token, 259_200).await;
    Ok(display_only(info))
}

/// Encerra a sessão: apaga o cookie HttpOnly (o cliente já limpa a exibição).
#[server]
pub async fn customer_logout() -> Result<(), ServerFnError> {
    write_cookie("", 0).await;
    Ok(())
}
