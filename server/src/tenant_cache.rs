//! Cache `subdomínio → company_id` para a resolução de tenant (§13).
//!
//! O middleware de tenant (`middleware/tenant.rs`) resolvia o `company_id`
//! com 1 SELECT no Postgres em TODA requisição — e o SSR do catálogo faz ~5
//! chamadas à API por página, multiplicando o custo na rota de MAIOR volume.
//! O mapeamento muda raro (criar/renomear/remover empresa), então um cache
//! com TTL curto elimina quase todos esses round-trips sem código de
//! invalidação: uma renomeação/remoção reflete em no máximo `TTL`.
//!
//! Também cacheia o NEGATIVO (subdomínio inexistente) com TTL menor, para que
//! bots sondando subdomínios aleatórios não batam no banco a cada tentativa —
//! e para uma empresa recém-criada resolver logo (o negativo expira rápido).

use std::collections::HashMap;
use std::future::Future;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use uuid::Uuid;

use letaf_core::error::CoreError;

/// TTL do resultado POSITIVO (subdomínio → company_id existente).
const POSITIVE_TTL: Duration = Duration::from_secs(60);
/// TTL do resultado NEGATIVO (subdomínio inexistente).
const NEGATIVE_TTL: Duration = Duration::from_secs(20);
/// Teto de entradas antes de uma varredura preguiçosa (anti-crescimento por
/// sondagem de subdomínios distintos).
const GC_THRESHOLD: usize = 4096;

pub struct TenantCache {
    entries: Mutex<HashMap<String, (Option<Uuid>, Instant)>>,
}

impl TenantCache {
    pub fn new() -> Self {
        Self { entries: Mutex::new(HashMap::new()) }
    }

    fn ttl(val: &Option<Uuid>) -> Duration {
        if val.is_some() { POSITIVE_TTL } else { NEGATIVE_TTL }
    }

    /// Resolve o `company_id` do `subdomain` pelo cache; em miss/expirado chama
    /// `loader` (o SELECT no banco) e memoiza. O lock NUNCA é mantido através
    /// do `.await` do loader (§concorrência AI_RULES).
    pub async fn resolve<F, Fut>(
        &self,
        subdomain: &str,
        loader: F,
    ) -> Result<Option<Uuid>, CoreError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Option<Uuid>, CoreError>>,
    {
        let now = Instant::now();
        // 1) Leitura sob lock (sem await).
        if let Ok(map) = self.entries.lock() {
            if let Some((val, at)) = map.get(subdomain) {
                if now.duration_since(*at) < Self::ttl(val) {
                    return Ok(*val);
                }
            }
        }
        // 2) Miss/expirado: carrega do banco SEM segurar o lock.
        let val = loader().await?;
        // 3) Grava sob lock; poda preguiçosa quando o mapa cresce.
        if let Ok(mut map) = self.entries.lock() {
            if map.len() >= GC_THRESHOLD {
                map.retain(|_, (v, at)| now.duration_since(*at) < Self::ttl(v));
            }
            map.insert(subdomain.to_string(), (val, now));
        }
        Ok(val)
    }
}

impl Default for TenantCache {
    fn default() -> Self {
        Self::new()
    }
}
