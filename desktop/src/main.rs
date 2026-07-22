mod alarm;
mod context;
mod format;
mod nav_perms;
mod print;
mod repository;
mod session;
mod sync;
mod ui;
mod update;

slint::include_modules!();

use std::env;
use std::sync::{Arc, LazyLock};

use reqwest::StatusCode;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use tokio::sync::{Notify, RwLock};
use uuid::Uuid;

use letaf_core::addon::service::AddonService;
use letaf_core::addon_group::service::AddonGroupService;
use letaf_core::banner::service::BannerService;
use letaf_core::coupon::service::CouponService;
use letaf_core::customer_address::service::CustomerAddressService;
use letaf_core::auth::service::AuthService;
use letaf_core::business_hours::service::BusinessHoursService;
use letaf_core::category::service::CategoryService;
use letaf_core::job_role::service::JobRoleService;
use letaf_core::subcategory::service::SubcategoryService;
use letaf_core::company::service::CompanyService;
use letaf_core::customer::service::CustomerService;
use letaf_core::order::service::OrderService;
use letaf_core::printer::service::PrinterService;
use letaf_core::product::service::ProductService;

pub(crate) static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("Failed to build HTTP client")
});

use crate::context::DesktopState;
use crate::repository::addon::SqliteAddonRepository;
use crate::repository::addon_group::SqliteAddonGroupRepository;
use letaf_core::subscription::service::SubscriptionService;
use letaf_core::payment_method::service::PaymentMethodService;
use crate::repository::banner::SqliteBannerRepository;
use crate::repository::payment_method::SqlitePaymentMethodRepository;
use crate::repository::subscription::SqliteSubscriptionRepository;
use crate::repository::coupon::SqliteCouponRepository;
use crate::repository::customer_address::SqliteCustomerAddressRepository;
use crate::repository::auth::SqliteUserRepository;
use crate::repository::business_hours::SqliteBusinessHoursRepository;
use crate::repository::company::SqliteCompanyRepository;
use crate::repository::customer::SqliteCustomerRepository;
use crate::repository::order::SqliteOrderRepository;
use crate::repository::product::SqliteProductRepository;
use crate::repository::category::SqliteCategoryRepository;
use crate::repository::job_role::SqliteJobRoleRepository;
use crate::repository::subcategory::SqliteSubcategoryRepository;
use crate::session::SessionStore;
use crate::sync::health::HealthChecker;
use crate::sync::status::SyncStatusHandle;
use crate::sync::worker::SyncWorker;

/// Instala um panic hook que imprime mensagem + backtrace sempre.
///
/// Substitui o antigo set_var de RUST_BACKTRACE (que exigia bloco
/// não-seguro) por uma solução 100% safe (AI_RULES.md). Ajuda a
/// diagnosticar panics do Slint (e.g. "Recursion detected") sem exigir
/// re-execução com a env var.
fn install_backtrace_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        eprintln!("{info}");
        eprintln!(
            "stack backtrace:\n{}",
            std::backtrace::Backtrace::force_capture()
        );
    }));
}

/// Ponto de entrada do desktop.
///
/// Regras aplicadas (AI_RULES.md §1, §3, §7, §8):
/// - Inicializa state → sync worker → UI Slint
/// - Cada responsabilidade em sua camada
fn main() {
    // Carrega variáveis do `.env` da raiz (LETAF_SERVER_URL, etc.) —
    // igual ao server. Sem isto, o desktop cairia no default e
    // apontaria para a porta errada da API.
    dotenvy::dotenv().ok();

    // Backtrace por padrão em panics, via panic hook seguro (sem
    // bloco não-seguro de env::set_var) — AI_RULES.md.
    install_backtrace_panic_hook();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,sctk_adwaita=off")),
        )
        .init();
    tracing::info!("LETAF Desktop - starting...");

    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    let state = rt.block_on(init_state());

    tracing::info!("SQLite ready, company_id = {}", state.company_id());

    let auth_token = Arc::new(RwLock::new(None::<String>));
    let sync_notify = Arc::new(Notify::new());
    // Notify disparado pelo worker ao fim de cada ciclo — a UI usa
    // para atualizar o rótulo "Sincronizado/Aguardando sync" sem o
    // operador precisar sair/voltar na tela.
    // `watch` (não `Notify`): ~7 telas escutam o fim de ciclo. Com
    // `notify_one` só UMA acordava por ciclo (rodízio) e a UI parecia
    // "congelada"; com `watch` cada tela tem seu cursor e todas acordam.
    let (sync_cycle_done, sync_cycle_rx) = tokio::sync::watch::channel(0u64);
    // Notify dedicado ao recompute dos badges da sidebar (ouvinte único),
    // separado do `cycle_done` para nunca perder um ciclo.
    let badges_dirty = Arc::new(Notify::new());
    let server_url = env::var("LETAF_SERVER_URL")
        .unwrap_or_else(|_| "http://localhost:3001".into());

    // Restaura sessão salva (token + company_id)
    let has_session = rt.block_on(restore_session(&state, &auth_token, &server_url));

    // Super admin de plataforma é ONLINE-only: pausa o SyncWorker da loja
    // ANTES de subi-lo (senão o 1º ciclo leva 401 em /sync/* e força logout).
    if has_session {
        let (_is_admin, is_super_admin, _perms) = rt.block_on(state.session.load_perms());
        if is_super_admin {
            state.set_sync_paused(true);
        }
    }

    let initial_last_pull_at = rt.block_on(state.session.load_last_pull_at());
    let worker = SyncWorker::new(state.clone(), server_url.clone(), auth_token.clone(), sync_notify.clone(), sync_cycle_done, badges_dirty.clone(), initial_last_pull_at);
    rt.spawn(async move { worker.start().await });
    tracing::info!("SyncWorker spawned");

    // Heartbeat de rede: detecta queda em ~5–8 s, independente do ciclo de sync (§7).
    let health = HealthChecker::new(server_url.clone(), state.sync_status.clone());
    rt.spawn(async move { health.start().await });
    tracing::info!("HealthChecker spawned");

    let window = MainWindow::new().expect("Failed to create UI");
    if rt.block_on(state.session.load_dark_mode()) {
        window.global::<Theme>().set_dark_mode(true);
    }
    // Versão exibida na tela de login (apenas o número, ex.: v0.1.0).
    window.set_app_version(slint::SharedString::from(concat!("v", env!("CARGO_PKG_VERSION"))));
    // Subdomínio do último login (se já houve algum) — identifica o
    // estabelecimento no rodapé da tela de login.
    if let Some(sd) = rt.block_on(state.session.load_subdomain()) {
        window.set_login_subdomain(slint::SharedString::from(sd));
    }
    if has_session {
        window.set_logged_in(true);
        // Restaura a gating de navegação salva (RBAC §11) — funciona
        // offline, sem depender do servidor.
        let (is_admin, is_super_admin, perms) = rt.block_on(state.session.load_perms());
        window.set_nav_perms(nav_perms::nav_perms_from(is_admin, is_super_admin, &perms));
        if let Some(name) = rt.block_on(state.session.load_user_name()) {
            window.set_user_name(slint::SharedString::from(name));
        }
        // Abre a primeira aba acessível ao operador restaurado — adiada
        // para depois do primeiro frame (evita recursão de layout no
        // startup ao montar telas que usam `root.height`, ex.: PDV).
        window.set_pending_initial_tab(slint::SharedString::from(
            nav_perms::first_accessible_tab(is_admin, is_super_admin, &perms),
        ));
        tracing::info!("Session restored, skipping login");
    } else {
        // Pré-preenche só o email (a senha não é mais persistida — §11).
        // Se o token de sessão ainda for válido, o ramo `has_session`
        // acima nem chega aqui; expirado, o usuário redigita a senha.
        let rem_email = rt.block_on(state.session.load_remember_email()).unwrap_or_default();
        if !rem_email.is_empty() {
            window.set_login_email(slint::SharedString::from(rem_email));
            window.set_login_remember_me(true);
        }
    }
    // Verificador de atualização (background, não bloqueia a UI; §7).
    // Checa /app/version e, havendo versão nova, aciona o UpdateModal.
    {
        let checker = update::UpdateChecker::new(server_url.clone(), window.as_weak());
        rt.spawn(async move { checker.start().await });
        // "Atualizar agora": auto-update (baixa, valida sha256, substitui
        // o binário e reinicia) em background.
        let apply_weak = window.as_weak();
        let apply_handle = rt.handle().clone();
        window.on_apply_update(move || {
            let Some(ui) = apply_weak.upgrade() else { return };
            let url = ui.get_update_url().to_string();
            let sha = ui.get_update_sha256().to_string();
            let ui_weak = ui.as_weak();
            apply_handle.spawn(async move {
                update::apply_update(url, sha, ui_weak).await;
            });
        });
        // Fallback manual: abre a URL do binário no navegador/SO.
        let open_weak = window.as_weak();
        window.on_open_update_url(move || {
            if let Some(ui) = open_weak.upgrade() {
                update::open_url(ui.get_update_url().as_str());
            }
        });
    }
    ui::setup_callbacks(
        &window,
        &state,
        rt.handle(),
        sync_notify,
        sync_cycle_rx,
        badges_dirty,
        auth_token,
        server_url,
    );
    // Carrega só dados do cabeçalho do menu lateral (logo + nome do estabelecimento).
    // As demais listas (produtos, categorias, etc.) são carregadas sob demanda
    // quando o usuário navega para a aba correspondente — evita exibir badges
    // de contagem antes que o usuário interaja com a área.
    window.invoke_refresh_business_hours();
    // Aba inicial = Dashboard. O carregamento das telas é sob demanda
    // via `navigate` (clique no menu), mas a aba de destino não recebe
    // esse evento na abertura — então a tela vinha vazia até o primeiro
    // ciclo de sync. Dispara o refresh inicial dos cards/séries do
    // Dashboard aqui (equivalente ao navigate("dashboard")).
    window.invoke_dashboard_refresh();
    // Pedidos: também pré-populado para o badge da sidebar aparecer
    // logo no boot e para Pedidos abrir cheio se o user navegar.
    window.invoke_refresh_orders();
    // Impressoras: carrega a lista no boot (não depende do operador
    // entrar em Configurações — o resolver de impressão por kind nos
    // pedidos consulta o banco direto, mas a UI já fica populada).
    window.invoke_refresh_printers();
    // PDV: carrega produtos + categorias no boot. Mesma justificativa
    // — quando o operador abre a aba PDV, a grid já está populada.
    window.invoke_pdv_refresh();
    // Caixa: status pré-carregado para que o modal de bloqueio do
    // PDV (renderizado quando `cash-summary.open == false`) já reflita
    // a realidade na primeira interação.
    window.invoke_cash_refresh();
    // Financeiro: KPIs + fluxo de caixa + lista de contas a pagar/receber.
    // Sem pré-carga a tela ficava em branco até o primeiro ciclo de sync.
    window.invoke_finance_refresh();
    // Assinatura: card de plano atual + lista de faturas. Pré-carrega
    // para que ASSINATURA → PLANO & COBRANÇA já reflita o estado real.
    window.invoke_subscription_refresh();
    // Clientes: lista mestre pra que o usuário já veja a base ao
    // navegar — a carteira do cliente selecionado é carregada por
    // listener separado quando ele clicar numa linha.
    window.invoke_refresh_customers();

    // Atualiza `live-window-height` (usada pelo PDV pra ancorar
    // cart-bottom no fim da janela) periodicamente. Slint não
    // expõe `on_resize` em todas as plataformas, então usamos um
    // Timer simples que dispara a cada 250ms e ajusta. O custo é
    // desprezível e cobre redimensionamento do usuário em tempo
    // real. Property externa quebra a cadeia de layout buggy do
    // Slint que estava colapsando alturas no card do carrinho.
    {
        let weak = window.as_weak();
        let timer = Box::leak(Box::new(slint::Timer::default()));
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(250),
            move || {
                if let Some(ui) = weak.upgrade() {
                    let win = ui.window();
                    let size = win.size();
                    let scale = win.scale_factor();
                    let logical_h = (size.height as f32) / scale;
                    if (ui.get_live_window_height() - logical_h).abs() > 0.5 {
                        ui.set_live_window_height(logical_h);
                    }
                }
            },
        );
    }

    window.run().expect("UI failed");
}

/// Restaura sessão anterior do SQLite.
///
/// Regras aplicadas (AI_RULES.md §7.1, §10, §11):
/// - Offline-first: sessão persistida localmente
/// - Acesso ao banco via SessionStore (repository)
/// - Quando online, valida token contra `/auth/me` (detecta `company_id`
///   stale após reset do servidor, usuário removido, token expirado).
/// - Quando offline, mantém sessão (fallback offline — §7.1).
///
/// Retorna true se havia sessão salva e ela é válida (ou o servidor está offline).
async fn restore_session(
    state: &DesktopState,
    auth_token: &RwLock<Option<String>>,
    server_url: &str,
) -> bool {
    let token = state.session.load_token().await;
    let company_id = state.session.load_company_id().await;

    let (token, cid) = match (token, company_id) {
        (Some(t), Some(c)) => (t, c),
        _ => return false,
    };

    match validate_token_online(server_url, &token).await {
        TokenStatus::Valid => {
            *auth_token.write().await = Some(token);
            state.set_company_id(cid);
            tracing::info!("Restored session: company_id = {cid}");
            true
        }
        TokenStatus::Rejected => {
            tracing::warn!("Saved token rejected by server; clearing session");
            state.session.clear().await;
            false
        }
        TokenStatus::Offline => {
            *auth_token.write().await = Some(token);
            state.set_company_id(cid);
            tracing::info!("Server offline; restored session offline (company_id = {cid})");
            true
        }
    }
}

enum TokenStatus { Valid, Rejected, Offline }

async fn validate_token_online(server_url: &str, token: &str) -> TokenStatus {
    let url = format!("{server_url}/auth/me");
    match HTTP_CLIENT.get(&url).bearer_auth(token).send().await {
        Ok(resp) if resp.status().is_success() => TokenStatus::Valid,
        Ok(resp) if resp.status() == StatusCode::UNAUTHORIZED
            || resp.status() == StatusCode::FORBIDDEN => TokenStatus::Rejected,
        Ok(resp) => {
            tracing::warn!("Unexpected status from /auth/me: {}", resp.status());
            TokenStatus::Offline
        }
        Err(e) => {
            tracing::info!("Server unreachable ({e}); offline mode");
            TokenStatus::Offline
        }
    }
}

/// Inicializa pool SQLite, executa migrations, monta repos → services → state.
///
/// Regras aplicadas (AI_RULES.md §1, §5, §7, §9):
/// - Desktop usa SQLite
/// - Sistema funciona offline
/// - Services encapsulam repositories
async fn init_state() -> DesktopState {
    let connect_opts = SqliteConnectOptions::new()
        .filename("letaf.db")
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        // `busy_timeout` é CRÍTICO em WAL com múltiplas conexões: SQLite
        // ainda serializa escritas, e sem timeout uma escrita concorrente
        // (UI + SyncWorker em paralelo) falha imediatamente com
        // SQLITE_BUSY. Com 5s, a transação espera o writer atual.
        .busy_timeout(std::time::Duration::from_secs(5))
        // FK desabilitado intencionalmente: no sync offline-first entidades podem
        // chegar fora de ordem (ex.: Order antes do Customer). Consistência é
        // garantida pelo protocolo de sync (§7 — last-write-wins por updated_at).
        .foreign_keys(false);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(connect_opts)
        .await
        .expect("Failed to connect to SQLite");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run SQLite migrations");

    tracing::info!("SQLite migrations applied");

    let product_service = Arc::new(ProductService::new(
        Arc::new(SqliteProductRepository::new(pool.clone())),
    ));
    let auth_service = Arc::new(AuthService::new(
        Arc::new(SqliteUserRepository::new(pool.clone())),
    ));
    let company_service = Arc::new(CompanyService::new(
        Arc::new(SqliteCompanyRepository::new(pool.clone())),
    ));
    let customer_service = Arc::new(CustomerService::new(
        Arc::new(SqliteCustomerRepository::new(pool.clone())),
    ));
    // Repo de Category compartilhado entre CategoryService e SubcategoryService
    // (este último precisa validar que `category_id` pertence à empresa).
    let business_hours_service = Arc::new(BusinessHoursService::new(
        Arc::new(SqliteBusinessHoursRepository::new(pool.clone())),
    ));
    let category_repo = Arc::new(SqliteCategoryRepository::new(pool.clone()));
    let category_service = Arc::new(CategoryService::new(category_repo.clone()));
    let job_role_service = Arc::new(JobRoleService::new(Arc::new(
        SqliteJobRoleRepository::new(pool.clone()),
    )));
    let subcategory_service = Arc::new(SubcategoryService::new(
        Arc::new(SqliteSubcategoryRepository::new(pool.clone())),
        category_repo,
    ));
    // Gestão de caixa: service compõe os dois repos (sessões + movimentos).
    // Construído ANTES do `order_service` porque o PDV precisa injetar
    // o `CashService` para lançar movimentos de venda automaticamente.
    let cash_service = Arc::new(letaf_core::cash::service::CashService::new(
        Arc::new(repository::cash_session::SqliteCashSessionRepository::new(pool.clone())),
        Arc::new(repository::cash_movement::SqliteCashMovementRepository::new(pool.clone())),
    ));
    let order_service = Arc::new(
        OrderService::new(
            Arc::new(SqliteOrderRepository::new(pool.clone())),
            product_service.clone(),
        )
        .with_cash_service(cash_service.clone()),
    );
    let addon_group_repo = Arc::new(SqliteAddonGroupRepository::new(pool.clone()));
    let addon_group_service = Arc::new(AddonGroupService::new(addon_group_repo.clone()));
    let addon_service = Arc::new(AddonService::new(
        Arc::new(SqliteAddonRepository::new(pool.clone())),
        addon_group_repo,
    ));
    let banner_service = Arc::new(BannerService::new(
        Arc::new(SqliteBannerRepository::new(pool.clone())),
    ));
    let coupon_service = Arc::new(CouponService::new(
        Arc::new(SqliteCouponRepository::new(pool.clone())),
    ));
    let customer_address_service = Arc::new(CustomerAddressService::new(
        Arc::new(SqliteCustomerAddressRepository::new(pool.clone())),
    ));
    // Impressoras locais (per-device, sem sync). Mesmo padrão dos
    // demais services — service envolve um repository SQLite.
    let printer_service = Arc::new(PrinterService::new(
        Arc::new(repository::printer::SqlitePrinterRepository::new(pool.clone())),
    ));
    // Fase 11: finanças (contas a pagar/receber + categorias).
    let finance_category_service = Arc::new(
        letaf_core::finance_category::service::FinanceCategoryService::new(Arc::new(
            repository::finance_category::SqliteFinanceCategoryRepository::new(pool.clone()),
        )),
    );
    let finance_service = Arc::new(letaf_core::finance::service::FinanceService::new(
        Arc::new(repository::finance::SqliteFinanceRepository::new(pool.clone())),
    ));
    // Fase 12: carteira do cliente — sync completo (multi-device).
    let wallet_service = Arc::new(letaf_core::wallet::service::WalletService::new(
        Arc::new(repository::wallet::SqliteWalletRepository::new(pool.clone())),
    ));
    // Assinatura: plano + faturas. Catálogo de planos é constante no
    // service; quando o super-admin existir, vira tabela `plans` sync.
    let subscription_service = Arc::new(SubscriptionService::new(
        Arc::new(SqliteSubscriptionRepository::new(pool.clone())),
    ));
    let payment_method_service = Arc::new(PaymentMethodService::new(
        Arc::new(SqlitePaymentMethodRepository::new(pool.clone())),
    ));

    let session = Arc::new(SessionStore::new(pool.clone()));
    let company_id = resolve_company_id(&company_service, &session).await;

    // Seed de assinatura padrão (plano Mensal + 5 faturas históricas)
    // na primeira execução offline. Idempotente em boots subsequentes.
    let today_seed = chrono::Local::now().date_naive();
    if let Err(e) = subscription_service.ensure_seed(company_id, today_seed).await {
        tracing::warn!("Subscription seed falhou: {e}");
    }

    // Seed de categorias financeiras padrão na primeira execução.
    // Idempotente: não duplica em boots subsequentes.
    match finance_category_service.seed_defaults(company_id).await {
        Ok(0) => {} // já tinha categorias
        Ok(n) => tracing::info!("FinanceCategory seed: {n} categorias criadas"),
        Err(e) => tracing::warn!("FinanceCategory seed falhou: {e}"),
    }

    // Alarme de novos pedidos — instâncias compartilhadas via Arc.
    // Player abre a thread de áudio agora (rodio); falha graciosamente
    // se não houver device. Watcher inicia vazio; o `seed` acontece em
    // `ui::alarm::init_alarm` quando o app carrega a lista de pedidos.
    // `alarm_signal` é o canal SyncWorker → UI (observer task).
    let alarm_watcher = Arc::new(alarm::AlarmWatcher::new());
    let alarm_player = Arc::new(alarm::AlarmPlayer::new());
    let alarm_signal = Arc::new(Notify::new());

    DesktopState::new(
        company_id,
        product_service,
        auth_service,
        company_service,
        customer_service,
        business_hours_service,
        category_service,
        job_role_service,
        subcategory_service,
        order_service,
        addon_group_service,
        addon_service,
        banner_service,
        coupon_service,
        customer_address_service,
        printer_service,
        cash_service,
        finance_category_service,
        finance_service,
        wallet_service,
        subscription_service,
        payment_method_service,
        Arc::new(repository::reconcile::SqliteReconcileRepository::new(pool.clone())),
        session,
        SyncStatusHandle::new(),
        alarm_watcher,
        alarm_player,
        alarm_signal,
    )
}

/// Carrega ou cria a empresa local no SQLite.
///
/// Regras aplicadas (AI_RULES.md §4, §10):
/// - Desktop representa UMA empresa, company_id fixo localmente
/// - Acesso via service (nunca banco direto)
///
/// Na primeira execução, cria uma empresa padrão.
/// Nas seguintes, reutiliza a existente.
///
/// Prefere a empresa da SESSÃO salva (o tenant efetivamente logado neste
/// device) quando ela existe localmente — evita que os seeds
/// (`ensure_seed`/`seed_defaults`) rodem no tenant ERRADO se o SQLite acumulou
/// mais de uma empresa (troca de conta no mesmo computador). Sem sessão, cai
/// na primeira empresa; sem nenhuma, cria a padrão (§11).
async fn resolve_company_id(service: &CompanyService, session: &SessionStore) -> Uuid {
    let companies = service.find_all().await
        .expect("Failed to load companies");

    if let Some(saved) = session.load_company_id().await {
        if let Some(company) = companies.iter().find(|c| c.id == saved) {
            tracing::info!("Company da sessão: {} ({})", company.name, company.id);
            return company.id;
        }
    }

    if let Some(company) = companies.first() {
        tracing::info!("Loaded existing company: {} ({})", company.name, company.id);
        return company.id;
    }

    let company = service
        .create("Minha Empresa".into(), "local".into())
        .await
        .expect("Failed to create local company");

    tracing::info!("Created local company: {} ({})", company.name, company.id);
    company.id
}
