//! Rate limiting in-house (sem dependência) para os endpoints de
//! autenticação — freia brute force de credenciais e spam de e-mail de
//! recuperação (AI_RULES §11). Janela deslizante por IP, em memória.
//!
//! Escopo deliberado: só as rotas de login/forgot-password. NÃO é um limiter
//! de propósito geral. Em memória = por-instância (não sobrevive a restart nem
//! compartilha entre réplicas) — é a 1ª barreira, complementada pelo custo do
//! bcrypt (cost 13) em cada tentativa.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::request::Parts;

use crate::context::AppState;
use crate::error::ServerError;

/// Limitador de janela deslizante por chave (IP). Guarda os instantes das
/// tentativas recentes de cada IP e recusa quando passam de `max` dentro de
/// `window`.
pub struct RateLimiter {
    hits: Mutex<HashMap<IpAddr, Vec<Instant>>>,
    max: usize,
    window: Duration,
}

impl RateLimiter {
    pub fn new(max: usize, window: Duration) -> Self {
        Self { hits: Mutex::new(HashMap::new()), max, window }
    }

    /// Registra uma tentativa do `ip` e devolve `true` se está DENTRO do
    /// limite (permitida) ou `false` se excedeu. Poda os instantes fora da
    /// janela; remove a chave quando não sobra tentativa recente (memória
    /// limitada aos IPs ativos na janela). Falha de lock → permite (o
    /// limiter é defesa-em-profundidade, não pode derrubar o login).
    pub fn check(&self, ip: IpAddr) -> bool {
        let now = Instant::now();
        let Ok(mut map) = self.hits.lock() else { return true; };
        let times = map.entry(ip).or_default();
        let allowed = allow(times, now, self.window, self.max);
        if times.is_empty() {
            map.remove(&ip);
        }
        allowed
    }
}

/// Lógica pura da janela deslizante (testável sem relógio real): poda os
/// instantes anteriores a `now - window`; se ainda cabe (`< max`), registra
/// `now` e permite; senão recusa (sem registrar, para não estender o bloqueio
/// indefinidamente sob ataque).
fn allow(times: &mut Vec<Instant>, now: Instant, window: Duration, max: usize) -> bool {
    times.retain(|&t| now.duration_since(t) < window);
    if times.len() < max {
        times.push(now);
        true
    } else {
        false
    }
}

/// IP do cliente para o rate limit. Por padrão usa o IP do socket
/// (`ConnectInfo`); quando `config.trust_proxy` está ligado, usa o 1º IP do
/// `X-Forwarded-For` (deploy atrás de proxy reverso). Sem nenhuma fonte,
/// cai num IP fixo (agrupa desconhecidos — degrada para limite global).
pub struct ClientIp(pub IpAddr);

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = ServerError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if state.config.trust_proxy {
            if let Some(ip) = parts
                .headers
                .get("x-forwarded-for")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.split(',').next())
                .and_then(|s| s.trim().parse::<IpAddr>().ok())
            {
                return Ok(ClientIp(ip));
            }
        }
        if let Some(ConnectInfo(addr)) = parts.extensions.get::<ConnectInfo<SocketAddr>>() {
            return Ok(ClientIp(addr.ip()));
        }
        Ok(ClientIp(IpAddr::V4(Ipv4Addr::UNSPECIFIED)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allow_permite_ate_o_maximo_e_recusa_depois() {
        let now = Instant::now();
        let mut t = Vec::new();
        // max=3, janela=60s: 3 permitidas, 4ª recusada.
        assert!(allow(&mut t, now, Duration::from_secs(60), 3));
        assert!(allow(&mut t, now, Duration::from_secs(60), 3));
        assert!(allow(&mut t, now, Duration::from_secs(60), 3));
        assert!(!allow(&mut t, now, Duration::from_secs(60), 3));
        assert_eq!(t.len(), 3, "tentativa recusada não é registrada");
    }

    #[test]
    fn allow_libera_apos_janela_expirar() {
        let base = Instant::now();
        let mut t = Vec::new();
        assert!(allow(&mut t, base, Duration::from_secs(60), 1));
        // Mesmo instante → excede.
        assert!(!allow(&mut t, base, Duration::from_secs(60), 1));
        // 61s depois → a antiga saiu da janela → permite de novo.
        let later = base + Duration::from_secs(61);
        assert!(allow(&mut t, later, Duration::from_secs(60), 1));
        assert_eq!(t.len(), 1);
    }
}
