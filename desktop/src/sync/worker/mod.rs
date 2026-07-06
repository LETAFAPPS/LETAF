use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::NaiveDateTime;
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::sync::{Notify, RwLock};
use uuid::Uuid;

use letaf_core::error::CoreError;

use crate::context::DesktopState;

const SYNC_INTERVAL_SECS: u64 = 30;

/// Worker de sincronização em background.
///
/// Regras aplicadas (AI_RULES.md §7):
/// - Offline-first: toda escrita ocorre primeiro no SQLite
/// - Worker busca dados com synced = false
/// - Tenta reenviar periodicamente
/// - Não bloqueia a UI (roda em tokio::task separada)
/// - Nenhuma falha de rede impede o uso do sistema
///
/// Usa services (não repos) para acessar dados (§1, §10).
/// O token JWT é compartilhado com a UI via Arc<RwLock>.
/// Estado de progresso publicado em `state.sync_status` (§7).
pub struct SyncWorker {
    state: DesktopState,
    server_url: String,
    http: Client,
    auth_token: Arc<RwLock<Option<String>>>,
    notify: Arc<Notify>,
    /// Disparado ao final de cada ciclo (sucesso ou falha) — a UI usa
    /// para refrescar telas que dependem do flag `synced` (ex.: rótulo
    /// "Sincronizado" / "Aguardando sync" na master-detail de Produtos).
    /// `notify_waiters` acorda todos os listeners; se ninguém estiver
    /// escutando, é no-op.
    cycle_done: Arc<Notify>,
    /// Notify DEDICADO ao recompute dos badges da sidebar (um só ouvinte).
    /// Separado do `cycle_done` (7 ouvintes com `notify_one` rotativo) para
    /// nunca perder um ciclo — dá o "tempo real" dos badges.
    badges_dirty: Arc<Notify>,
    last_pull_at: Mutex<Option<NaiveDateTime>>,
    /// Marcado true quando alguma chamada HTTP do ciclo atual falha por motivo
    /// de rede (timeout, DNS, conexão recusada) — diferenciando de erros de
    /// status HTTP (4xx/5xx) que indicam servidor acessível mas com problema.
    network_failed: Mutex<bool>,
}

mod push;
mod pull;

impl SyncWorker {
    pub fn new(
        state: DesktopState,
        server_url: String,
        auth_token: Arc<RwLock<Option<String>>>,
        notify: Arc<Notify>,
        cycle_done: Arc<Notify>,
        badges_dirty: Arc<Notify>,
        initial_last_pull_at: Option<NaiveDateTime>,
    ) -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to build SyncWorker HTTP client");
        Self {
            state,
            server_url,
            http,
            auth_token,
            notify,
            cycle_done,
            badges_dirty,
            last_pull_at: Mutex::new(initial_last_pull_at),
            network_failed: Mutex::new(false),
        }
    }

    /// Marca que houve falha de rede neste ciclo (timeout/DNS/conexão).
    fn flag_network_failure(&self) {
        if let Ok(mut g) = self.network_failed.lock() {
            *g = true;
        }
    }

    /// Inicia o loop de sincronização.
    /// Deve ser chamado via tokio::spawn (não bloqueia UI — §7.8).
    ///
    /// Regras aplicadas (AI_RULES.md §7.3, §7.4, §7.5):
    /// - Tenta sincronizar imediatamente quando notificado (§7.4)
    /// - Fallback: reenvia periodicamente a cada 30s (§7.5)
    pub async fn start(self) {
        tracing::info!("SyncWorker started (interval: {SYNC_INTERVAL_SECS}s)");
        loop {
            self.run_cycle().await;
            tokio::select! {
                _ = self.notify.notified() => {
                    tracing::debug!("SyncWorker: immediate sync triggered");
                }
                _ = tokio::time::sleep(Duration::from_secs(SYNC_INTERVAL_SECS)) => {}
            }
        }
    }

    /// Executa um ciclo completo de sincronização.
    ///
    /// Regras aplicadas (AI_RULES.md §7, §8):
    /// - Atualiza `state.sync_status` para a UI consumir (sem await na UI)
    /// - `online` reflete sucesso de rede; `pending_count` é recontado ao fim
    async fn run_cycle(&self) {
        // Super admin de plataforma é online-only: não sincroniza dados de
        // loja (as rotas /sync/* rejeitariam com 401 e forçariam logout).
        if self.state.sync_paused() {
            return;
        }
        let Some(token) = self.read_token_or_skip().await else { return };

        // Início do ciclo
        if let Ok(mut g) = self.network_failed.lock() { *g = false; }
        self.state.sync_status.mark_syncing();

        self.run_pushes(&token).await;
        if let Err(e) = self.pull_all(&token).await {
            tracing::warn!("Pull sync error: {e}");
        }

        // Fim do ciclo: consolida status
        let had_network_fail = self.network_failed.lock().map(|g| *g).unwrap_or(false);
        let online = !had_network_fail;
        let pending = self.count_pending().await;
        let now = chrono::Utc::now().naive_utc();
        self.state.sync_status.mark_finished(online, now, pending);
        // Notifica a UI para refrescar telas dependentes do `synced`
        // (master-detail de Produtos atualiza o rótulo abaixo do nome).
        //
        // `notify_one` (em vez de `notify_waiters`) bufferiza um permit
        // quando o listener está ocupado — `notify_waiters` PERDIA
        // notificações se o listener estivesse no meio do trabalho
        // anterior (find_unsynced + invoke_from_event_loop). Resultado
        // visível: produto com imagem ficava "Aguardando sync" porque
        // o ciclo do worker fechava enquanto o listener ainda
        // processava o ciclo anterior.
        self.cycle_done.notify_one();
        // Badges da sidebar (ouvinte único e dedicado) — recompute a cada
        // ciclo, refletindo mudanças locais e vindas do pull em tempo real.
        self.badges_dirty.notify_one();
    }

    /// Lê o token JWT atual ou pula o ciclo (publicando só `pending_count`).
    async fn read_token_or_skip(&self) -> Option<String> {
        let guard = self.auth_token.read().await;
        if let Some(t) = guard.as_ref() {
            return Some(t.clone());
        }
        drop(guard);
        tracing::debug!("No auth token, skipping sync cycle");
        let pending = self.count_pending().await;
        self.state.sync_status.set_pending(pending);
        None
    }

    /// Executa os pushes (desktop → servidor) logando erros individuais.
    ///
    /// **Ordem crítica (AI_RULES.md §7, §11):**
    /// `addon_groups` → `addons` → `products` — produtos referenciam
    /// grupos via `product_addon_groups` (FK no servidor); empurrar
    /// produto antes do grupo violaria a integridade referencial e o
    /// servidor rejeitaria com erro 500.
    /// O mesmo vale para `categories` antes de `subcategories` antes de
    /// `products` (subcategory referencia category; product referencia
    /// subcategory).
    async fn run_pushes(&self, token: &str) {
        if let Err(e) = self.sync_companies(token).await {
            tracing::warn!("Company sync error: {e}");
        }
        // Funções antes dos usuários (usuário referencia job_role_id).
        if let Err(e) = self.sync_job_roles(token).await {
            tracing::warn!("JobRole sync error: {e}");
        }
        if let Err(e) = self.sync_users(token).await {
            tracing::warn!("User sync error: {e}");
        }
        if let Err(e) = self.sync_customers(token).await {
            tracing::warn!("Customer sync error: {e}");
        }
        if let Err(e) = self.sync_categories(token).await {
            tracing::warn!("Category sync error: {e}");
        }
        if let Err(e) = self.sync_subcategories(token).await {
            tracing::warn!("Subcategory sync error: {e}");
        }
        if let Err(e) = self.sync_addon_groups(token).await {
            tracing::warn!("AddonGroup sync error: {e}");
        }
        if let Err(e) = self.sync_addons(token).await {
            tracing::warn!("Addon sync error: {e}");
        }
        // Produtos por último entre os "cadastros" porque podem
        // referenciar todos os anteriores (subcategoria + grupos).
        if let Err(e) = self.sync_products(token).await {
            tracing::warn!("Product sync error: {e}");
        }
        if let Err(e) = self.sync_orders(token).await {
            tracing::warn!("Order sync error: {e}");
        }
        if let Err(e) = self.sync_business_hours(token).await {
            tracing::warn!("BusinessHours sync error: {e}");
        }
        // Banners podem referenciar produtos via `item_id`. Empurrar
        // depois de products garante que a FK lógica não fique órfã
        // após o sync (servidor não tem FK CASCADE — vide migration).
        if let Err(e) = self.sync_banners(token).await {
            tracing::warn!("Banner sync error: {e}");
        }
        if let Err(e) = self.sync_coupons(token).await {
            tracing::warn!("Coupon sync error: {e}");
        }
        // Endereços referenciam clientes via `customer_id`: empurrar
        // depois de customers para a FK lógica não ficar órfã.
        if let Err(e) = self.sync_customer_addresses(token).await {
            tracing::warn!("CustomerAddress sync error: {e}");
        }
        // Sessões de caixa antes dos movimentos (FK lógica session_id).
        if let Err(e) = self.sync_cash_sessions(token).await {
            tracing::warn!("CashSession sync error: {e}");
        }
        if let Err(e) = self.sync_cash_movements(token).await {
            tracing::warn!("CashMovement sync error: {e}");
        }
        // Categorias financeiras antes das entradas (FK lógica
        // category_id). Sem hard FK no SQLite/PG, mas mantém a ordem
        // pra evitar pull onde a entrada apareceria sem a categoria.
        if let Err(e) = self.sync_finance_categories(token).await {
            tracing::warn!("FinanceCategory sync error: {e}");
        }
        if let Err(e) = self.sync_finance_entries(token).await {
            tracing::warn!("FinanceEntry sync error: {e}");
        }
        // Wallet accounts antes dos movements pela FK lógica
        // `account_id`.
        if let Err(e) = self.sync_wallet_accounts(token).await {
            tracing::warn!("WalletAccount sync error: {e}");
        }
        if let Err(e) = self.sync_wallet_movements(token).await {
            tracing::warn!("WalletMovement sync error: {e}");
        }
        // Assinatura: independente das demais entidades — empurra
        // mudança de plano / forma de pagamento.
        if let Err(e) = self.sync_subscriptions(token).await {
            tracing::warn!("Subscription sync error: {e}");
        }
        if let Err(e) = self.sync_subscription_invoices(token).await {
            tracing::warn!("SubscriptionInvoice sync error: {e}");
        }
        if let Err(e) = self.sync_payment_methods(token).await {
            tracing::warn!("PaymentMethod sync error: {e}");
        }
    }

    /// Conta itens com `synced = false` em todos os domínios.
    ///
    /// Regras aplicadas (AI_RULES.md §7.2, §10):
    /// - Somatório de `find_unsynced` via services (sem SQL direto)
    /// - Falhas de leitura logam e contam como 0 para o domínio (não bloqueia)
    async fn count_pending(&self) -> u32 {
        let cid = self.state.company_id();
        let mut total: u32 = 0;
        macro_rules! add {
            ($fut:expr, $label:literal) => {
                match $fut.await {
                    Ok(items) => total = total.saturating_add(items.len() as u32),
                    Err(e) => tracing::debug!("count_pending {}: {e}", $label),
                }
            };
        }
        add!(self.state.product_service.find_unsynced(cid),       "products");
        add!(self.state.auth_service.find_unsynced(cid),          "users");
        add!(self.state.job_role_service.find_unsynced(cid),      "job_roles");
        add!(self.state.company_service.find_unsynced(),          "companies");
        add!(self.state.customer_service.find_unsynced(cid),      "customers");
        add!(self.state.category_service.find_unsynced(cid),      "categories");
        add!(self.state.subcategory_service.find_unsynced(cid),   "subcategories");
        add!(self.state.order_service.find_unsynced(cid),         "orders");
        add!(self.state.business_hours_service.find_unsynced(cid),"business_hours");
        add!(self.state.addon_group_service.find_unsynced(cid),   "addon_groups");
        add!(self.state.addon_service.find_unsynced(cid),         "addons");
        add!(self.state.banner_service.find_unsynced(cid),        "banners");
        add!(self.state.coupon_service.find_unsynced(cid),        "coupons");
        add!(self.state.customer_address_service.find_unsynced(cid), "customer_addresses");
        add!(self.state.cash_service.find_unsynced_sessions(cid),    "cash_sessions");
        add!(self.state.cash_service.find_unsynced_movements(cid),   "cash_movements");
        add!(self.state.finance_category_service.find_unsynced(cid), "finance_categories");
        add!(self.state.finance_service.find_unsynced(cid),          "finance_entries");
        add!(self.state.wallet_service.find_unsynced_accounts(cid),  "wallet_accounts");
        add!(self.state.wallet_service.find_unsynced_movements(cid), "wallet_movements");
        add!(self.state.subscription_service.find_unsynced_subscriptions(cid), "subscriptions");
        add!(self.state.subscription_service.find_unsynced_invoices(cid), "subscription_invoices");
        add!(self.state.payment_method_service.find_unsynced(cid), "payment_methods");
        total
    }

    // ── Pull (servidor → desktop) ──────────────────────────

    /// Executa pull de todas as entidades atualizadas desde last_pull_at.
    ///
    /// Regras aplicadas (AI_RULES.md §7.5, §7.6, §8):
    /// - Cada chamada de pull delegada a método dedicado
    /// - Orquestração centralizada, sem duplicação
    /// - **Avanço conservador do cursor**: `last_pull_at` só é atualizado
    ///   quando TODAS as entidades pularam com sucesso. Se uma falha,
    ///   o cursor não avança — no próximo ciclo, todas as entidades são
    ///   re-puxadas desde `since`. Sem isso, uma falha de rede numa
    ///   entidade fazia o cursor avançar com base nas que sucederam,
    ///   pulando registros das que falharam (perda de dados silenciosa).
    async fn pull_all(&self, token: &str) -> Result<(), CoreError> {
        // Lock tratado graciosamente: se o Mutex estiver envenenado
        // (panic de outra task), NÃO derruba o sync — apenas parte
        // do zero (re-puxa tudo; o upsert é idempotente por LWW).
        let since = match self.last_pull_at.lock() {
            Ok(g) => g.unwrap_or_default(),
            Err(p) => p.into_inner().unwrap_or_default(),
        };
        let mut max_ts = since;
        // Se QUALQUER pull falhar, não avançamos `last_pull_at` —
        // próximo ciclo re-puxa todas as entidades desde `since`.
        // Upsert é idempotente por LWW (§7.7), então re-puxar não
        // corrompe dados.
        let mut any_failed = false;
        macro_rules! try_pull {
            ($call:expr, $label:literal) => {
                match $call.await {
                    Ok(ts) => { if ts > max_ts { max_ts = ts; } }
                    Err(e) => {
                        any_failed = true;
                        tracing::warn!("pull {} falhou (será re-tentado): {e}", $label);
                    }
                }
            };
        }
        try_pull!(self.pull_companies(token, since, max_ts), "companies");
        try_pull!(self.pull_job_roles(token, since, max_ts), "job_roles");
        try_pull!(self.pull_users(token, since, max_ts), "users");
        try_pull!(self.pull_customers(token, since, max_ts), "customers");
        try_pull!(self.pull_categories(token, since, max_ts), "categories");
        try_pull!(self.pull_subcategories(token, since, max_ts), "subcategories");
        try_pull!(self.pull_addon_groups(token, since, max_ts), "addon_groups");
        try_pull!(self.pull_addons(token, since, max_ts), "addons");
        try_pull!(self.pull_products(token, since, max_ts), "products");
        try_pull!(self.pull_orders(token, since, max_ts), "orders");
        try_pull!(self.pull_business_hours(token, since, max_ts), "business_hours");
        try_pull!(self.pull_banners(token, since, max_ts), "banners");
        try_pull!(self.pull_coupons(token, since, max_ts), "coupons");
        try_pull!(self.pull_customer_addresses(token, since, max_ts), "customer_addresses");
        try_pull!(self.pull_cash_sessions(token, since, max_ts), "cash_sessions");
        try_pull!(self.pull_cash_movements(token, since, max_ts), "cash_movements");
        try_pull!(self.pull_finance_categories(token, since, max_ts), "finance_categories");
        try_pull!(self.pull_finance_entries(token, since, max_ts), "finance_entries");
        try_pull!(self.pull_wallet_accounts(token, since, max_ts), "wallet_accounts");
        try_pull!(self.pull_wallet_movements(token, since, max_ts), "wallet_movements");
        try_pull!(self.pull_subscriptions(token, since, max_ts), "subscriptions");
        try_pull!(self.pull_subscription_invoices(token, since, max_ts), "subscription_invoices");
        try_pull!(self.pull_payment_methods(token, since, max_ts), "payment_methods");

        if any_failed {
            // Mantém o cursor — re-puxa tudo no próximo ciclo.
            // Upsert idempotente garante que não duplica nem corrompe.
            tracing::debug!(
                "Pull com falhas parciais; last_pull_at preservado ({since}) para re-tentativa"
            );
        } else if max_ts > since {
            // Recua o cursor 1µs: um registro gravado no servidor com
            // `updated_at` IGUAL ao máximo deste ciclo, mas logo após o
            // snapshot do pull, seria excluído para sempre por
            // `updated_at > since` no próximo ciclo. O recuo re-inclui o
            // limite no pull seguinte — idempotente (upsert LWW), fecha a
            // janela de perda silenciosa (§7.6).
            let cursor = max_ts - chrono::Duration::microseconds(1);
            match self.last_pull_at.lock() {
                Ok(mut g) => *g = Some(cursor),
                Err(p) => *p.into_inner() = Some(cursor),
            }
            self.state.session.save_last_pull_at(cursor).await;
            tracing::debug!("Pull complete, last_pull_at = {cursor}");
        }
        Ok(())
    }

    /// GET genérico para pull de entidades do servidor.
    ///
    /// Regras aplicadas (AI_RULES.md §7.6, §11):
    /// - 401 indica JWT expirado ou inválido: limpa o token e retorna erro específico
    ///   para interromper o ciclo de sync e forçar re-login.
    async fn fetch_pull<T: DeserializeOwned>(
        &self,
        token: &str,
        endpoint: &str,
        since: NaiveDateTime,
    ) -> Result<Vec<T>, CoreError> {
        let url = format!("{}{}?since={}", self.server_url, endpoint, since.format("%Y-%m-%dT%H:%M:%S%.f"));
        let resp = self.http.get(&url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| {
                self.flag_network_failure();
                CoreError::Repository(format!("Pull {endpoint}: {e}"))
            })?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            tracing::warn!("SyncWorker: JWT rejeitado (401) em pull {endpoint}; limpando token");
            *self.auth_token.write().await = None;
            return Err(CoreError::Unauthorized("JWT expirado durante sync pull".into()));
        }

        if !resp.status().is_success() {
            return Err(CoreError::Repository(format!("Pull {endpoint}: status {}", resp.status())));
        }

        resp.json::<Vec<T>>().await
            .map_err(|e| CoreError::Repository(format!("Pull {endpoint} decode: {e}")))
    }

    // ── Push (desktop → servidor) ──────────────────────────

    /// Envia uma entidade ao servidor via HTTP POST.
    ///
    /// Retorna true se o servidor aceitou (2xx).
    /// Falhas de rede são logadas e retornam false (§7.6 — resiliência).
    /// 401 limpa o token de autenticação para forçar re-login.
    async fn send_one<T: Serialize>(
        &self,
        token: &str,
        endpoint: &str,
        id: Uuid,
        entity: &T,
    ) -> bool {
        let url = format!("{}{}", self.server_url, endpoint);
        match self.http.post(&url).bearer_auth(token).json(entity).send().await {
            Ok(resp) if resp.status().is_success() => {
                tracing::debug!("Synced {endpoint} {id}");
                true
            }
            Ok(resp) if resp.status() == reqwest::StatusCode::UNAUTHORIZED => {
                tracing::warn!("SyncWorker: JWT rejeitado (401) em push {endpoint}; limpando token");
                *self.auth_token.write().await = None;
                false
            }
            Ok(resp) => {
                tracing::warn!("Sync {endpoint} {id}: status {}", resp.status());
                false
            }
            Err(e) => {
                tracing::warn!("Sync {endpoint} {id}: {e}");
                self.flag_network_failure();
                false
            }
        }
    }
}

