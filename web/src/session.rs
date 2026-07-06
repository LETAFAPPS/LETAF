//! Sessão do cliente final: token JWT + identidade, compartilhada por
//! contexto e persistida em localStorage. AI_RULES §11 (frontend burro):
//! o cliente só GUARDA o token e o envia; quem valida credenciais e
//! emite/zera o JWT é o backend. As `#[server]` fns abaixo fazem proxy
//! para a API (forward de `Host`), igual ao catálogo — o navegador nunca
//! fala direto com a API.

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

    /// Token atual (sem rastrear — uso em handlers/envios).
    pub fn token(&self) -> Option<String> {
        self.0.with_untracked(|s| s.as_ref().map(|i| i.token.clone()))
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

/// POST /customer/login (proxy). Backend valida e devolve o JWT.
#[server]
pub async fn customer_login(email: String, password: String) -> Result<SessionInfo, ServerFnError> {
    let host = tenant_host().await?;
    crate::api::customer_login(&host, &email, &password)
        .await
        .map_err(ServerFnError::new)
}

/// POST /customer/register (proxy). Backend cria o cliente e devolve o JWT.
#[server]
pub async fn customer_register(
    name: String,
    email: String,
    phone: String,
    password: String,
) -> Result<SessionInfo, ServerFnError> {
    let host = tenant_host().await?;
    crate::api::customer_register(&host, &name, &email, &phone, &password)
        .await
        .map_err(ServerFnError::new)
}
