use std::time::Duration;

use reqwest::Client;

use crate::sync::status::SyncStatusHandle;

/// Intervalo entre pings consecutivos ao servidor.
const CHECK_INTERVAL_SECS: u64 = 5;

/// Timeout total (e de conexão) para o ping de health.
/// Curto o suficiente para detectar cabo desconectado rapidamente.
const HEALTH_TIMEOUT_SECS: u64 = 3;

/// Heartbeat leve que mantém o flag `online` do `SyncStatus` atualizado.
///
/// Regras aplicadas (AI_RULES.md §7, §8, §11):
/// - Detecta queda de rede em ~5–8 s sem depender do ciclo de sync (30 s).
/// - GET no endpoint público `/health` — não exige autenticação.
/// - Roda em tokio task separada; não bloqueia UI nem SyncWorker.
/// - Não modifica `phase`, `pending_count` ou `last_sync_at` — só `online`.
pub struct HealthChecker {
    server_url: String,
    status: SyncStatusHandle,
    http: Client,
}

impl HealthChecker {
    pub fn new(server_url: String, status: SyncStatusHandle) -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(HEALTH_TIMEOUT_SECS))
            .connect_timeout(Duration::from_secs(HEALTH_TIMEOUT_SECS))
            .build()
            .expect("Failed to build HealthChecker HTTP client");
        Self { server_url, status, http }
    }

    /// Loop principal: ping → atualiza status → dorme.
    pub async fn start(self) {
        let url = format!("{}/health", self.server_url);
        tracing::info!("HealthChecker started (interval: {CHECK_INTERVAL_SECS}s)");
        loop {
            let online = self.ping(&url).await;
            self.status.set_online(online);
            tokio::time::sleep(Duration::from_secs(CHECK_INTERVAL_SECS)).await;
        }
    }

    /// Faz um GET ao endpoint de health. Retorna true se 2xx.
    async fn ping(&self, url: &str) -> bool {
        match self.http.get(url).send().await {
            Ok(resp) => resp.status().is_success(),
            Err(e) => {
                tracing::debug!("HealthChecker: ping falhou ({e})");
                false
            }
        }
    }
}
