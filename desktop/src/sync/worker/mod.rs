use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::NaiveDateTime;
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::sync::{watch, Notify, RwLock};
use uuid::Uuid;

use letaf_core::error::CoreError;

use crate::sync::status::CLOCK_SKEW_TOLERANCE_SECS;

use crate::context::DesktopState;

const SYNC_INTERVAL_SECS: u64 = 30;
/// A reconciliação (anti-entropia, §7) roda no 1º ciclo (login) e a cada N
/// ciclos como rede de segurança — não a cada ciclo, pois busca o manifesto
/// de todas as entidades. Com intervalo de 30s, N=10 ≈ a cada 5 min.
const RECONCILE_EVERY_N_CYCLES: u64 = 10;

/// Entidades puxadas a cada ciclo, na ordem de dependência (pais antes de
/// filhos por FK lógica). Cada uma tem um cursor de pull PRÓPRIO (§7) — ver
/// `SyncWorker::cursors`.
const PULL_ENTITIES: [&str; 26] = [
    "companies", "job_roles", "users", "customers", "categories", "subcategories",
    "addon_groups", "addons", "products", "orders", "business_hours", "banners",
    "coupons", "customer_addresses", "cash_sessions", "cash_movements",
    "finance_categories", "finance_entries", "wallet_accounts", "wallet_movements",
    "treasury_accounts", "treasury_movements", "subscriptions",
    "subscription_invoices", "payment_methods", "stock_movements",
];

/// Tamanho da página do pull paginado (keyset). Abaixo do teto do servidor
/// (`MAX_PAGE_LIMIT=1000`) — margem para não ser cortado.
const PULL_PAGE_LIMIT: i64 = 500;

/// Expõe o cursor keyset `(updated_at, id)` de um item puxado, para o
/// `fetch_pull_paged` avançar a paginação. Implementado pelos tipos das
/// entidades grandes (produtos, pedidos, ledgers).
pub(super) trait PullCursor {
    fn pull_cursor(&self) -> (NaiveDateTime, Uuid);
}

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
    /// Contador de ciclos difundido para TODAS as telas ao final de cada
    /// ciclo (sucesso ou falha). É um `watch` — e não um `Notify` — porque
    /// há ~7 ouvintes: `notify_one` acordaria só UM por ciclo (rodízio, cada
    /// tela atualizando a cada 7 ciclos) e `notify_waiters` PERDERIA o aviso
    /// para quem estivesse ocupado processando o ciclo anterior. Com `watch`
    /// cada ouvinte tem seu próprio cursor: ninguém é preterido e nada se
    /// perde (ciclos múltiplos coalescem num refresh só, que é o desejado).
    cycle_done: watch::Sender<u64>,
    /// Notify DEDICADO ao recompute dos badges da sidebar (um só ouvinte).
    badges_dirty: Arc<Notify>,
    /// Cursor de pull POR ENTIDADE (§7): cada entidade avança independente
    /// pelo próprio `max(updated_at)`. Um cursor global (único) fazia um
    /// registro gravado após o pull da entidade X — mas com `updated_at` menor
    /// que o máximo de uma entidade Y puxada depois no mesmo ciclo — cair
    /// abaixo do cursor e ser pulado no incremental (só a reconciliação
    /// recuperava, ~5 min). Persistimos o MÍNIMO do mapa (conservador no
    /// restart; upsert idempotente).
    cursors: Mutex<HashMap<&'static str, NaiveDateTime>>,
    /// Contador de ciclos — dispara a reconciliação no 1º ciclo e a cada
    /// `RECONCILE_EVERY_N_CYCLES` (anti-entropia, §7).
    reconcile_tick: std::sync::atomic::AtomicU64,
    /// Marcado true quando alguma chamada HTTP do ciclo atual falha por motivo
    /// de rede (timeout, DNS, conexão recusada) — diferenciando de erros de
    /// status HTTP (4xx/5xx) que indicam servidor acessível mas com problema.
    network_failed: Mutex<bool>,
    /// Entidades cujo PULL falhou neste ciclo (403 de permissão, 500,
    /// decode). Sem isto, a UI mostrava "Sincronizado" com uma entidade
    /// congelada há horas — a falha só existia no log.
    pull_failed: std::sync::atomic::AtomicU32,
    /// Registros pulados por não serem legíveis por este binário.
    poison_count: std::sync::atomic::AtomicU32,
    /// Diferença entre o relógio local e o do servidor, em segundos
    /// (positivo = esta máquina adiantada). Ver `observe_server_clock`.
    clock_skew: std::sync::atomic::AtomicI64,
    /// Empresa a que os cursores em memória pertencem (ver
    /// `sync_cursors_with_tenant`).
    cursor_company: Mutex<uuid::Uuid>,
    /// Registros recusados pelo servidor e seu backoff (ver `send_one`).
    quarantine: Mutex<std::collections::HashMap<uuid::Uuid, Quarentena>>,
    /// Pedidos cuja edição feita NESTE terminal foi descartada porque
    /// outro terminal editou o mesmo pedido depois (last-write-wins).
    /// Não é erro de sync — é conflito de edição, e o operador precisa
    /// saber para reabrir o pedido.
    push_superseded: std::sync::atomic::AtomicU32,
    /// Nº de registros rejeitados com erro de cliente (4xx, exceto 401) no
    /// ciclo corrente — dado preso que não sobe sem intervenção. Reiniciado a
    /// cada ciclo; publicado no `SyncStatus` para a UI mostrar o estado de erro.
    push_rejected: std::sync::atomic::AtomicU32,
}

mod push;
mod pull;
mod reconcile;

/// Ponto de partida do worker: cursores gravados e a empresa a que eles
/// pertencem (§7 — cursor é por tenant).
pub(crate) struct SyncBootstrap {
    pub(crate) cursors: Vec<(String, NaiveDateTime)>,
    pub(crate) company_id: uuid::Uuid,
}

impl SyncWorker {
    pub fn new(
        state: DesktopState,
        server_url: String,
        auth_token: Arc<RwLock<Option<String>>>,
        notify: Arc<Notify>,
        cycle_done: watch::Sender<u64>,
        badges_dirty: Arc<Notify>,
        bootstrap: SyncBootstrap,
    ) -> Self {
        let SyncBootstrap { cursors: initial_cursors, company_id: initial_company } = bootstrap;
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
            // Cada entidade retoma do PRÓPRIO cursor gravado; as que nunca
            // sincronizaram começam na época.
            cursors: Mutex::new(
                PULL_ENTITIES
                    .iter()
                    .map(|&e| {
                        let saved = initial_cursors
                            .iter()
                            .find(|(k, _)| k == e)
                            .map(|(_, ts)| *ts)
                            .unwrap_or_default();
                        (e, saved)
                    })
                    .collect(),
            ),
            reconcile_tick: std::sync::atomic::AtomicU64::new(0),
            network_failed: Mutex::new(false),
            push_rejected: std::sync::atomic::AtomicU32::new(0),
            pull_failed: std::sync::atomic::AtomicU32::new(0),
            poison_count: std::sync::atomic::AtomicU32::new(0),
            clock_skew: std::sync::atomic::AtomicI64::new(0),
            cursor_company: Mutex::new(initial_company),
            quarantine: Mutex::new(std::collections::HashMap::new()),
            push_superseded: std::sync::atomic::AtomicU32::new(0),
        }
    }

    /// Marca que um registro veio ilegível no pull (versão nova do
    /// servidor, dado corrompido) e foi pulado.
    fn flag_poison(&self) {
        self.poison_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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

        // Trocou de empresa? Recarrega os cursores dela antes de qualquer
        // push/pull.
        self.sync_cursors_with_tenant().await;

        // Início do ciclo
        if let Ok(mut g) = self.network_failed.lock() { *g = false; }
        self.push_rejected.store(0, std::sync::atomic::Ordering::Relaxed);
        self.pull_failed.store(0, std::sync::atomic::Ordering::Relaxed);
        self.push_superseded.store(0, std::sync::atomic::Ordering::Relaxed);
        self.poison_count.store(0, std::sync::atomic::Ordering::Relaxed);
        self.state.sync_status.mark_syncing();

        // Reconciliação (anti-entropia §7): no 1º ciclo (login) e a cada N
        // ciclos. Roda ANTES do push/pull para que os reparos que ela agenda
        // (marcar unsynced / resetar cursor) sejam efetivados neste mesmo
        // ciclo — push reenvia o que falta no servidor; pull re-puxa o que
        // falta no local.
        let tick = self.reconcile_tick.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if tick.is_multiple_of(RECONCILE_EVERY_N_CYCLES) {
            self.reconcile_all(&token).await;
        }

        self.run_pushes(&token).await;
        if let Err(e) = self.pull_all(&token).await {
            tracing::warn!("Pull sync error: {e}");
        }

        // Fim do ciclo: consolida status
        let had_network_fail = self.network_failed.lock().map(|g| *g).unwrap_or(false);
        let online = !had_network_fail;
        let pending = self.count_pending().await;
        let rejected = self.push_rejected.load(std::sync::atomic::Ordering::Relaxed);
        let now = chrono::Utc::now().naive_utc();
        let pull_failed = self.pull_failed.load(std::sync::atomic::Ordering::Relaxed);
        let poison = self.poison_count.load(std::sync::atomic::Ordering::Relaxed);
        let superseded = self
            .push_superseded
            .load(std::sync::atomic::Ordering::Relaxed);
        self.state.sync_status.mark_finished(crate::sync::status::CycleOutcome {
            online,
            last_sync_at: now,
            pending_count: pending,
            rejected_count: rejected,
            pull_failed_count: pull_failed,
            poison_count: poison + superseded,
            clock_skew_seconds: self.clock_skew.load(std::sync::atomic::Ordering::Relaxed),
        });
        // Notifica a UI para refrescar telas dependentes do `synced`
        // (master-detail de Produtos atualiza o rótulo abaixo do nome).
        //
        // Incrementa o contador: acorda TODAS as telas (cada uma com seu
        // próprio cursor no `watch`), inclusive as que estavam ocupadas
        // processando o ciclo anterior. `send_modify` não falha quando não
        // há nenhum receptor vivo.
        self.cycle_done.send_modify(|v| *v = v.wrapping_add(1));
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
        // Movimentos de estoque ANTES dos produtos (§7): o `apply_stock_movement`
        // do servidor só aplica o delta se o produto JÁ existe (senão 0 linhas).
        // Empurrando os movimentos primeiro, para um produto NOVO (ainda não
        // sincronizado) o delta é descartado e o INSERT do produto logo em
        // seguida carrega o estoque LÍQUIDO correto — evitando o double-count
        // (INSERT com o absoluto já decrementado + delta reaplicado). Para
        // produto EXISTENTE o delta aplica e o upsert do produto preserva o
        // estoque (não sobrescreve). Ambos os pushes vão no mesmo ciclo (rede
        // up→ambos, down→nenhum), então o caso realista fica correto. O desktop
        // não puxa/reaplica movimentos (estoque vem da linha do produto).
        if let Err(e) = self.sync_stock_movements(token).await {
            tracing::warn!("StockMovement sync error: {e}");
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
        // Carteira do estabelecimento (tesouraria) — singleton, sem
        // dependência das demais entidades.
        if let Err(e) = self.sync_treasury(token).await {
            tracing::warn!("Treasury sync error: {e}");
        }
        // Movimentos manuais depois da carteira (FK lógica `treasury_id`).
        if let Err(e) = self.sync_treasury_movements(token).await {
            tracing::warn!("TreasuryMovement sync error: {e}");
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
        add!(self.state.product_service.find_unsynced_stock_movements(cid), "stock_movements");
        add!(self.state.auth_service.find_unsynced(cid),          "users");
        add!(self.state.job_role_service.find_unsynced(cid),      "job_roles");
        // companies: conta SÓ a própria empresa — as demais nunca sobem
        // (ver `sync_companies`), então não podem inflar a fila para sempre.
        match self.state.company_service.find_unsynced().await {
            Ok(items) => {
                total = total.saturating_add(items.iter().filter(|c| c.id == cid).count() as u32)
            }
            Err(e) => tracing::debug!("count_pending companies: {e}"),
        }
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
        add!(self.state.treasury_service.find_unsynced(cid),         "treasury_accounts");
        add!(self.state.treasury_service.find_unsynced_movements(cid), "treasury_movements");
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
        // Cada entidade puxa desde o SEU cursor e o avança pelo SEU máximo,
        // independente das demais (fecha a janela do cursor global, §7). Uma
        // falha só congela o cursor daquela entidade — as outras avançam.
        let mut any_failed = false;
        macro_rules! try_pull {
            ($method:ident, $label:literal) => {{
                let since = self.cursor_of($label);
                match self.$method(token, since, since).await {
                    Ok(ts) if ts > since => {
                        // Recua 1µs: um registro com `updated_at` IGUAL ao
                        // máximo, gravado logo após o snapshot, seria excluído
                        // por `updated_at > since` no próximo ciclo. O recuo o
                        // re-inclui — idempotente (upsert LWW), §7.6.
                        //
                        // E limita ao AGORA: `updated_at` é carimbado pelo
                        // relógio de quem originou o registro. Um terminal
                        // com o relógio adiantado empurrava o cursor para o
                        // futuro e congelava a entidade até o relógio
                        // alcançar — nenhuma alteração legítima passava no
                        // `updated_at > since`.
                        // Limita ao AGORA (relógio adiantado de outro
                        // terminal congelaria a entidade), mas NUNCA
                        // retrocede: com o relógio local atrasado, o
                        // `min(now)` sozinho cravava o cursor em "agora" e
                        // a mesma janela era re-baixada a cada 30 s, para
                        // sempre.
                        let capped = ts.min(chrono::Utc::now().naive_utc())
                            - chrono::Duration::microseconds(1);
                        self.advance_cursor($label, capped.max(since));
                    }
                    Ok(_) => {}
                    Err(e) => {
                        any_failed = true;
                        self.pull_failed
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        tracing::warn!("pull {} falhou (será re-tentado): {e}", $label);
                    }
                }
            }};
        }
        try_pull!(pull_companies, "companies");
        try_pull!(pull_job_roles, "job_roles");
        try_pull!(pull_users, "users");
        try_pull!(pull_customers, "customers");
        try_pull!(pull_categories, "categories");
        try_pull!(pull_subcategories, "subcategories");
        try_pull!(pull_addon_groups, "addon_groups");
        try_pull!(pull_addons, "addons");
        try_pull!(pull_products, "products");
        try_pull!(pull_stock_movements, "stock_movements");
        try_pull!(pull_orders, "orders");
        try_pull!(pull_business_hours, "business_hours");
        try_pull!(pull_banners, "banners");
        try_pull!(pull_coupons, "coupons");
        try_pull!(pull_customer_addresses, "customer_addresses");
        try_pull!(pull_cash_sessions, "cash_sessions");
        try_pull!(pull_cash_movements, "cash_movements");
        try_pull!(pull_finance_categories, "finance_categories");
        try_pull!(pull_finance_entries, "finance_entries");
        try_pull!(pull_wallet_accounts, "wallet_accounts");
        try_pull!(pull_wallet_movements, "wallet_movements");
        try_pull!(pull_treasury_accounts, "treasury_accounts");
        try_pull!(pull_treasury_movements, "treasury_movements");
        try_pull!(pull_subscriptions, "subscriptions");
        try_pull!(pull_subscription_invoices, "subscription_invoices");
        try_pull!(pull_payment_methods, "payment_methods");

        // Persiste o cursor de CADA entidade. Antes salvava-se só o menor
        // de todos: uma entidade sem novidades (ou com pull negado por
        // permissão) mantinha o mínimo em 1970 e todo reinício re-baixava
        // a base inteira desde a época.
        let snapshot = self.cursor_snapshot();
        let pairs: Vec<(&str, chrono::NaiveDateTime)> =
            snapshot.iter().map(|(k, v)| (*k, *v)).collect();
        // Grava sob a empresa DONA do mapa (não `company_id()`): a troca
        // de tenant acontece em outra task e podia cair no meio do ciclo,
        // salvando os cursores da empresa antiga sob a chave da nova.
        let dona = match self.cursor_company.lock() {
            Ok(g) => *g,
            Err(p) => *p.into_inner(),
        };
        self.state.session.save_pull_cursors(dona, &pairs).await;
        if any_failed {
            tracing::debug!("Pull com falhas parciais; cursores das que falharam preservados");
        }
        Ok(())
    }

    /// Cursor de pull atual de uma entidade (default = época se ausente).
    fn cursor_of(&self, label: &str) -> NaiveDateTime {
        match self.cursors.lock() {
            Ok(g) => g.get(label).copied(),
            Err(p) => p.into_inner().get(label).copied(),
        }
        .unwrap_or_default()
    }

    /// Avança o cursor de uma entidade após um pull bem-sucedido.
    fn advance_cursor(&self, label: &'static str, ts: NaiveDateTime) {
        match self.cursors.lock() {
            Ok(mut g) => { g.insert(label, ts); }
            Err(p) => { p.into_inner().insert(label, ts); }
        }
    }

    /// Cópia do mapa de cursores (o que persistimos a cada ciclo).
    fn cursor_snapshot(&self) -> Vec<(&'static str, NaiveDateTime)> {
        match self.cursors.lock() {
            Ok(g) => g.iter().map(|(k, v)| (*k, *v)).collect(),
            Err(p) => p.into_inner().iter().map(|(k, v)| (*k, *v)).collect(),
        }
    }

    /// Detecta troca de EMPRESA e recarrega os cursores da nova.
    ///
    /// Os cursores são por tenant. Manter os da empresa anterior fazia o
    /// pull incremental da nova (`updated_at > agora`) não trazer NADA —
    /// a loja aparecia vazia e só a reconciliação, 5 min depois, trazia
    /// os dados (e em seguida repetia o repull integral para sempre,
    /// porque o cursor continuava no futuro).
    ///
    /// O worker detecta sozinho, no início do ciclo: não depende de o
    /// login/impersonation lembrar de avisar.
    async fn sync_cursors_with_tenant(&self) {
        let current = self.state.company_id();
        let previous = match self.cursor_company.lock() {
            Ok(g) => *g,
            Err(p) => *p.into_inner(),
        };
        if previous == current {
            return;
        }
        let saved = self.state.session.load_pull_cursors(current).await;
        let epoch = NaiveDateTime::default();
        let apply = |g: &mut std::collections::HashMap<&'static str, NaiveDateTime>| {
            for (label, ts) in g.iter_mut() {
                *ts = saved
                    .iter()
                    .find(|(k, _)| k == label)
                    .map(|(_, v)| *v)
                    .unwrap_or(epoch);
            }
        };
        match self.cursors.lock() {
            Ok(mut g) => apply(&mut g),
            Err(p) => apply(&mut p.into_inner()),
        }
        match self.cursor_company.lock() {
            Ok(mut g) => *g = current,
            Err(p) => *p.into_inner() = current,
        }
        tracing::info!("SyncWorker: empresa mudou ({previous} → {current}); cursores recarregados");
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
        self.get_pull_page(token, endpoint, &url).await
    }

    /// Pull PAGINADO por keyset `(updated_at, id)` — para entidades que crescem
    /// (ledgers, pedidos, produtos): puxa em páginas de até `PULL_PAGE_LIMIT`,
    /// evitando estourar o timeout HTTP num único GET gigante (§7, §13). O
    /// servidor ordena por `(updated_at, id)`; avançamos o cursor pelo ÚLTIMO
    /// item da página até vir uma página incompleta. `sync_upsert` é idempotente,
    /// então mesmo um reprocessamento de borda é seguro.
    async fn fetch_pull_paged<T: DeserializeOwned + PullCursor>(
        &self,
        token: &str,
        endpoint: &str,
        since: NaiveDateTime,
    ) -> Result<Vec<T>, CoreError> {
        let mut all: Vec<T> = Vec::new();
        let mut cur_ts = since;
        let mut cur_id = Uuid::nil();
        loop {
            let url = format!(
                "{}{}?since={}&after_id={}&limit={}",
                self.server_url,
                endpoint,
                cur_ts.format("%Y-%m-%dT%H:%M:%S%.f"),
                cur_id,
                PULL_PAGE_LIMIT,
            );
            let page: Vec<T> = self.get_pull_page(token, endpoint, &url).await?;
            let full = page.len() as i64 >= PULL_PAGE_LIMIT;
            if let Some(last) = page.last() {
                let (ts, id) = last.pull_cursor();
                cur_ts = ts;
                cur_id = id;
            }
            all.extend(page);
            if !full {
                break;
            }
        }
        Ok(all)
    }

    /// Compara o relógio desta máquina com o do servidor usando o header
    /// `Date` da resposta (§7.7).
    ///
    /// A resolução de conflito é last-write-wins sobre `updated_at`, que é
    /// carimbado pelo CLIENTE. Um terminal com o relógio adiantado ganha toda
    /// disputa e sobrescreve edições legítimas dos outros — e um atrasado
    /// perde todas, com as edições dele sumindo sem erro nenhum. Nada disso
    /// aparece como falha: o sync reporta sucesso o tempo todo.
    ///
    /// Detectamos e avisamos em vez de "corrigir" o timestamp no servidor:
    /// o cliente continuaria com o valor adiantado gravado localmente, a
    /// reconciliação veria "local mais novo" a cada ciclo e reempurraria o
    /// registro para sempre. A causa é o relógio da máquina — é ele que
    /// precisa ser acertado.
    ///
    /// Só observa: nunca ajusta timestamp e nunca falha o sync.
    fn observe_server_clock(&self, resp: &reqwest::Response) {
        let Some(server_now) = resp
            .headers()
            .get(reqwest::header::DATE)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| chrono::DateTime::parse_from_rfc2822(s).ok())
        else {
            return;
        };
        let skew = (chrono::Utc::now() - server_now.with_timezone(&chrono::Utc)).num_seconds();
        self.clock_skew
            .store(skew, std::sync::atomic::Ordering::Relaxed);
        if skew.abs() > CLOCK_SKEW_TOLERANCE_SECS {
            let sentido = if skew > 0 { "adiantado" } else { "atrasado" };
            tracing::warn!(
                "Relógio deste computador está {} {sentido} em relação ao servidor. \
                 A resolução de conflitos usa o horário local — acerte o relógio \
                 para não sobrescrever (ou perder) alterações de outros terminais.",
                fmt_duracao(skew.abs()),
            );
        }
    }

    /// GET de uma página do pull (envio + tratamento de status + decode).
    /// Base compartilhada por `fetch_pull` (tiro único) e `fetch_pull_paged`.
    async fn get_pull_page<T: DeserializeOwned>(
        &self,
        token: &str,
        endpoint: &str,
        url: &str,
    ) -> Result<Vec<T>, CoreError> {
        let resp = self.http.get(url)
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

        self.observe_server_clock(&resp);

        // Decodifica item a item. Antes era `json::<Vec<T>>()`: UM registro
        // com um valor que este binário não conhece (status novo, enum
        // novo — releases são por SO e independentes) invalidava a PÁGINA
        // INTEIRA, o cursor não avançava e a entidade congelava para
        // sempre, em silêncio. Agora o registro-veneno é pulado com log e
        // o resto da página entra.
        let raw = resp
            .json::<Vec<serde_json::Value>>()
            .await
            .map_err(|e| CoreError::Repository(format!("Pull {endpoint} decode: {e}")))?;
        let total = raw.len();
        let mut out = Vec::with_capacity(total);
        for value in raw {
            let id = value.get("id").and_then(|v| v.as_str()).unwrap_or("?").to_string();
            match serde_json::from_value::<T>(value) {
                Ok(item) => out.push(item),
                Err(e) => {
                    self.flag_poison();
                    tracing::warn!("Pull {endpoint}: registro {id} ignorado (decode): {e}");
                }
            }
        }
        Ok(out)
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
        // Quarentena: um registro que o servidor recusa por motivo
        // PERMANENTE (dado inválido, permissão, violação de restrição)
        // era reenviado a cada 30 s indefinidamente — banda e log
        // infinitos, e a fila mascarava qualquer registro novo preso.
        // Agora cada falha adia a próxima tentativa (backoff), sem
        // nunca desistir do dado: ele continua `synced=0` e volta a ser
        // tentado, só que com espaçamento crescente.
        if self.em_quarentena(id) {
            return false;
        }
        let url = format!("{}{}", self.server_url, endpoint);
        match self.http.post(&url).bearer_auth(token).json(entity).send().await {
            Ok(resp) if resp.status().is_success() => {
                tracing::debug!("Synced {endpoint} {id}");
                self.limpar_quarentena(id);
                true
            }
            Ok(resp) if resp.status() == reqwest::StatusCode::UNAUTHORIZED => {
                tracing::warn!("SyncWorker: JWT rejeitado (401) em push {endpoint}; limpando token");
                *self.auth_token.write().await = None;
                false
            }
            Ok(resp) => {
                let status = resp.status();
                // 4xx (exceto 401) = rejeição PERMANENTE: dado inválido ou
                // permissão insuficiente. Reenviar a cada ciclo não resolve —
                // conta como rejeitado para a UI sinalizar "há dado preso"
                // (§7.6), em vez de mascarar como pendente/sincronizado.
                // 5xx também conta: violação de restrição no servidor é
                // dado preso do mesmo jeito, e antes era invisível — a UI
                // mostrava só "pendente", indistinguível de "ainda não
                // enviei".
                if status.is_client_error() || status.is_server_error() {
                    self.push_rejected.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    self.registrar_falha(id);
                }
                tracing::warn!("Sync {endpoint} {id}: status {status}");
                false
            }
            Err(e) => {
                // Falha de REDE não entra em quarentena: o dado está bom,
                // o problema é a conexão.
                tracing::warn!("Sync {endpoint} {id}: {e}");
                self.flag_network_failure();
                false
            }
        }
    }

    /// Como o `send_one`, mas devolve também o corpo JSON da resposta.
    ///
    /// Usado pelo push de PEDIDOS: o servidor responde `applied: false`
    /// quando descartou a versão enviada por já ter uma mais nova. Antes
    /// isso era um 200 mudo e a edição daquele terminal sumia sem que
    /// ninguém soubesse.
    async fn send_one_with_body<T: Serialize>(
        &self,
        token: &str,
        endpoint: &str,
        id: Uuid,
        entity: &T,
    ) -> (bool, Option<serde_json::Value>) {
        if self.em_quarentena(id) {
            return (false, None);
        }
        let url = format!("{}{}", self.server_url, endpoint);
        match self.http.post(&url).bearer_auth(token).json(entity).send().await {
            Ok(resp) if resp.status().is_success() => {
                self.limpar_quarentena(id);
                let body = resp.json::<serde_json::Value>().await.ok();
                (true, body)
            }
            Ok(resp) if resp.status() == reqwest::StatusCode::UNAUTHORIZED => {
                tracing::warn!("SyncWorker: JWT rejeitado (401) em push {endpoint}; limpando token");
                *self.auth_token.write().await = None;
                (false, None)
            }
            Ok(resp) => {
                let status = resp.status();
                if status.is_client_error() || status.is_server_error() {
                    self.push_rejected.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    self.registrar_falha(id);
                }
                tracing::warn!("Sync {endpoint} {id}: status {status}");
                (false, None)
            }
            Err(e) => {
                tracing::warn!("Sync {endpoint} {id}: {e}");
                self.flag_network_failure();
                (false, None)
            }
        }
    }

    /// `true` enquanto o registro estiver aguardando o backoff.
    fn em_quarentena(&self, id: Uuid) -> bool {
        let agora = chrono::Utc::now().naive_utc();
        match self.quarantine.lock() {
            Ok(g) => g.get(&id).is_some_and(|q| q.proxima_tentativa > agora),
            Err(p) => p
                .into_inner()
                .get(&id)
                .is_some_and(|q| q.proxima_tentativa > agora),
        }
    }

    /// Conta a falha e agenda a próxima tentativa com backoff
    /// exponencial (1, 2, 4, 8… ciclos), com teto de 1 hora.
    fn registrar_falha(&self, id: Uuid) {
        let agora = chrono::Utc::now().naive_utc();
        let aplicar = |g: &mut std::collections::HashMap<Uuid, Quarentena>| {
            let q = g.entry(id).or_insert(Quarentena {
                tentativas: 0,
                proxima_tentativa: agora,
            });
            q.tentativas = q.tentativas.saturating_add(1);
            let ciclos = 1u32 << q.tentativas.min(7); // 2..128 ciclos
            let espera = (SYNC_INTERVAL_SECS as i64) * (ciclos as i64);
            q.proxima_tentativa = agora + chrono::Duration::seconds(espera.min(3600));
        };
        match self.quarantine.lock() {
            Ok(mut g) => aplicar(&mut g),
            Err(p) => aplicar(&mut p.into_inner()),
        }
    }

    /// Registro aceito: sai da quarentena.
    fn limpar_quarentena(&self, id: Uuid) {
        match self.quarantine.lock() {
            Ok(mut g) => { g.remove(&id); }
            Err(p) => { p.into_inner().remove(&id); }
        }
    }
}

/// Estado de backoff de um registro que o servidor recusou.
struct Quarentena {
    tentativas: u32,
    proxima_tentativa: NaiveDateTime,
}

/// "3 min" / "2 h 5 min" — usado só na mensagem de relógio fora de hora.
fn fmt_duracao(segundos: i64) -> String {
    let minutos = segundos / 60;
    match (minutos / 60, minutos % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

#[cfg(test)]
mod clock_tests {
    use super::fmt_duracao;

    #[test]
    fn formata_a_duracao_do_desvio() {
        assert_eq!(fmt_duracao(180), "3 min");
        assert_eq!(fmt_duracao(7200), "2 h");
        assert_eq!(fmt_duracao(7500), "2 h 5 min");
    }
}
