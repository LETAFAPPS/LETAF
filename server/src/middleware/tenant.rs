use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use uuid::Uuid;

use crate::context::AppState;
use crate::error::ServerError;

/// Contexto do tenant extraído do subdomínio.
///
/// Regras aplicadas (AI_RULES.md §2):
/// - Extrair subdomínio do header Host
/// - Resolver company_id via CompanyService (banco real)
/// - Injetar no contexto da requisição
/// - Nenhum endpoint funciona sem company_id
///
/// Uso como extractor axum:
/// ```ignore
/// async fn handler(tenant: TenantContext) -> impl IntoResponse { ... }
/// ```
#[derive(Debug, Clone)]
pub struct TenantContext {
    pub company_id: Uuid,
    pub subdomain: String,
}

impl FromRequestParts<AppState> for TenantContext {
    type Rejection = ServerError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let subdomain = extract_subdomain(parts)
            .ok_or(ServerError::TenantNotFound)?;

        let company_id = state
            .company_service
            .find_id_by_subdomain(&subdomain)
            .await
            .map_err(|_| ServerError::TenantNotFound)?
            .ok_or(ServerError::TenantNotFound)?;

        Ok(TenantContext {
            company_id,
            subdomain,
        })
    }
}

/// Extrai o subdomínio a partir do header Host.
///
/// Regras aplicadas (AI_RULES.md §5):
/// - Retorna `Some(subdomain)` apenas para hosts com subdomínio real
///   (ex.: `empresa1.seusite.com` → `empresa1`).
/// - Domínio principal (`seusite.com`), `www`, `localhost` e IPs retornam
///   `None`, sinalizando ausência de tenant (rotas ficam indisponíveis).
/// - Ignora a porta no Host (ex.: `empresa1.seusite.com:3000`).
fn extract_subdomain(parts: &Parts) -> Option<String> {
    let raw = parts.headers.get("host")?.to_str().ok()?;
    let host = raw.split(':').next()?.trim();
    if host.is_empty() || host == "localhost" {
        return None;
    }
    if host.parse::<std::net::IpAddr>().is_ok() {
        return None;
    }
    let segments: Vec<&str> = host.split('.').collect();
    let candidate = match segments.as_slice() {
        // Produção: <sub>.<dominio>.<tld>
        [sub, _, _, ..] => *sub,
        // Desenvolvimento: <sub>.localhost
        [sub, "localhost"] => *sub,
        _ => return None,
    };
    if candidate.is_empty() || candidate.eq_ignore_ascii_case("www") {
        return None;
    }
    Some(candidate.to_string())
}
