use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tokio::net::TcpListener;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};

use letaf_core::addon::service::AddonService;
use letaf_core::addon_group::service::AddonGroupService;
use letaf_core::auth::service::AuthService;
use letaf_core::password_reset::service::PasswordResetService;
use letaf_core::plan::service::PlanService;
use letaf_core::banner::service::BannerService;
use letaf_core::cash::service::CashService;
use letaf_core::coupon::service::CouponService;
use letaf_core::business_hours::service::BusinessHoursService;
use letaf_core::category::service::CategoryService;
use letaf_core::job_role::service::JobRoleService;
use letaf_core::subcategory::service::SubcategoryService;
use letaf_core::company::service::CompanyService;
use letaf_core::customer::service::CustomerService;
use letaf_core::product::service::ProductService;
use letaf_core::customer_address::service::CustomerAddressService;
use letaf_core::finance::service::FinanceService;
use letaf_core::finance_category::service::FinanceCategoryService;
use letaf_core::order::service::OrderService;
use letaf_core::wallet::service::WalletService;

use letaf_server::config::AppConfig;
use letaf_server::context::AppState;
use letaf_server::repository::addon::PgAddonRepository;
use letaf_server::repository::addon_group::PgAddonGroupRepository;
use letaf_core::subscription::service::SubscriptionService;
use letaf_core::subscription::card_billing::CardBillingService;
use letaf_core::subscription::pix_auto_billing::PixAutoBillingService;
use letaf_core::payment_gateway::service::PaymentService;
use letaf_core::payment_gateway::gateway::PaymentGateway;
use letaf_core::payment_gateway::card::CardGateway;
use letaf_core::payment_gateway::pix_auto::PixAutoGateway;
use letaf_core::payment_method::service::PaymentMethodService;
use letaf_server::config::EfiCardConfig;
use letaf_server::integrations::efi::{EfiCardClient, EfiClient};
use letaf_server::repository::banner::PgBannerRepository;
use letaf_server::repository::payment_charge::PgPaymentChargeRepository;
use letaf_server::repository::payment_method::PgPaymentMethodRepository;
use letaf_server::repository::subscription::PgSubscriptionRepository;
use letaf_server::repository::cash_movement::PgCashMovementRepository;
use letaf_server::repository::cash_session::PgCashSessionRepository;
use letaf_server::repository::coupon::PgCouponRepository;
use letaf_server::repository::auth::PgUserRepository;
use letaf_server::repository::password_reset::PgPasswordResetRepository;
use letaf_server::repository::plan::PgPlanRepository;
use letaf_server::repository::business_hours::PgBusinessHoursRepository;
use letaf_server::repository::company::PgCompanyRepository;
use letaf_server::repository::customer::PgCustomerRepository;
use letaf_server::repository::category::PgCategoryRepository;
use letaf_server::repository::job_role::PgJobRoleRepository;
use letaf_server::repository::subcategory::PgSubcategoryRepository;
use letaf_server::repository::product::PgProductRepository;
use letaf_server::repository::customer_address::PgCustomerAddressRepository;
use letaf_server::repository::finance::PgFinanceRepository;
use letaf_server::repository::finance_category::PgFinanceCategoryRepository;
use letaf_server::repository::order::PgOrderRepository;
use letaf_server::repository::wallet::PgWalletRepository;
use letaf_server::routes;

#[tokio::main]
async fn main() {
    // Filtro de log respeita `RUST_LOG` quando definido; senão usa
    // `info` para o crate da app e `warn` para libs ruidosas (sqlx,
    // hyper, tower). Evita logs DEBUG verbosos por padrão em prod.
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(
            "info,sqlx=warn,hyper=warn,tower_http=warn"
        ));
    tracing_subscriber::fmt().with_env_filter(filter).init();
    dotenvy::dotenv().ok();

    let config = AppConfig::from_env();
    let pool = connect_pool(&config.database_url).await;
    run_migrations(&pool).await;

    let state = build_state(pool, config.clone());
    // Garante empresa-plataforma + super admin default (painel do admin).
    // Idempotente; roda a cada boot. Ver routes::admin.
    letaf_server::routes::admin::ensure_platform_admin(&state).await;
    // Billing loop em background — emite cobrança recorrente PIX por
    // assinatura vencida. Não bloqueia o axum (§14B / Fase 14B).
    letaf_server::billing::start_billing_loop(state.clone());
    serve(state, &config).await;
}

/// Conecta ao PostgreSQL com pool configurado.
async fn connect_pool(database_url: &str) -> PgPool {
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(Duration::from_secs(600))
        .max_lifetime(Duration::from_secs(1800))
        .connect(database_url)
        .await
        .expect("Failed to connect to PostgreSQL");
    tracing::info!("Connected to PostgreSQL");
    pool
}

/// Executa as migrations da pasta `./migrations`.
async fn run_migrations(pool: &PgPool) {
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .expect("Failed to run migrations");
    tracing::info!("Migrations applied");
}

/// Monta repositories → services → AppState.
///
/// Regras aplicadas (AI_RULES.md §1, §9, §10):
/// - Services consomem repositories via trait (inversão de dependência)
/// - Repo de Category compartilhado entre CategoryService e SubcategoryService
///   (subcategoria valida que `category_id` pertence à empresa)
fn build_state(pool: PgPool, config: AppConfig) -> AppState {
    let product_service = Arc::new(ProductService::new(
        Arc::new(PgProductRepository::new(pool.clone())),
    ));
    let auth_service = Arc::new(AuthService::new(
        Arc::new(PgUserRepository::new(pool.clone())),
    ));
    let company_service = Arc::new(CompanyService::new(
        Arc::new(PgCompanyRepository::new(pool.clone())),
    ));
    let customer_service = Arc::new(CustomerService::new(
        Arc::new(PgCustomerRepository::new(pool.clone())),
    ));
    let business_hours_service = Arc::new(BusinessHoursService::new(
        Arc::new(PgBusinessHoursRepository::new(pool.clone())),
    ));
    let category_repo = Arc::new(PgCategoryRepository::new(pool.clone()));
    let category_service = Arc::new(CategoryService::new(category_repo.clone()));
    let job_role_service = Arc::new(JobRoleService::new(Arc::new(
        PgJobRoleRepository::new(pool.clone()),
    )));
    let subcategory_service = Arc::new(SubcategoryService::new(
        Arc::new(PgSubcategoryRepository::new(pool.clone())),
        category_repo,
    ));
    let customer_address_service = Arc::new(CustomerAddressService::new(
        Arc::new(PgCustomerAddressRepository::new(pool.clone())),
    ));
    let addon_group_repo = Arc::new(PgAddonGroupRepository::new(pool.clone()));
    let addon_group_service = Arc::new(AddonGroupService::new(addon_group_repo.clone()));
    let addon_service = Arc::new(AddonService::new(
        Arc::new(PgAddonRepository::new(pool.clone())),
        addon_group_repo,
    ));
    // OrderService recebe o AddonService para revalidar preço de
    // adicionais contra o catálogo (§11) no checkout web.
    let order_service = Arc::new(
        OrderService::new(
            Arc::new(PgOrderRepository::new(pool.clone())),
            product_service.clone(),
        )
        .with_addon_service(addon_service.clone()),
    );
    let banner_service = Arc::new(BannerService::new(
        Arc::new(PgBannerRepository::new(pool.clone())),
    ));
    let coupon_service = Arc::new(CouponService::new(
        Arc::new(PgCouponRepository::new(pool.clone())),
    ));
    let cash_service = Arc::new(CashService::new(
        Arc::new(PgCashSessionRepository::new(pool.clone())),
        Arc::new(PgCashMovementRepository::new(pool.clone())),
    ));
    let finance_category_service = Arc::new(FinanceCategoryService::new(
        Arc::new(PgFinanceCategoryRepository::new(pool.clone())),
    ));
    let finance_service = Arc::new(FinanceService::new(
        Arc::new(PgFinanceRepository::new(pool.clone())),
    ));
    let wallet_service = Arc::new(WalletService::new(
        Arc::new(PgWalletRepository::new(pool.clone())),
    ));
    let subscription_service = Arc::new(SubscriptionService::new(
        Arc::new(PgSubscriptionRepository::new(pool.clone())),
    ));
    let payment_method_service = Arc::new(PaymentMethodService::new(
        Arc::new(PgPaymentMethodRepository::new(pool.clone())),
    ));
    // Cliente Efi (API PIX, mTLS) construído UMA vez e compartilhado
    // entre o PIX imediato (PaymentService) e o Pix Automático (§11).
    // Quando EFI_* não estão setadas, fica None e os endpoints retornam
    // 503 ao invés de subir gateway inválido.
    let efi_client = build_efi_client(&config);
    let payment_service = efi_client.clone().map(|client| {
        let repo = Arc::new(PgPaymentChargeRepository::new(pool.clone()));
        Arc::new(PaymentService::new(repo, client as Arc<dyn PaymentGateway>))
    });
    // Cartão recorrente (API Cobranças da Efi). `None` quando EFI_PAYEE_CODE
    // / EFI_NOTIFICATION_URL não estão setadas — endpoints `/subscription/card*`
    // retornam 503 (§11).
    let card_billing = build_card_billing(&config, subscription_service.clone());
    // Pix Automático (reusa o EfiClient da API PIX). Habilitado sempre
    // que o EfiClient sobe. `notification_url` reaproveita a do cartão
    // quando configurada (o webhook PIX é registrado à parte na Efi).
    let pix_auto = efi_client.map(|client| {
        let notification_url = config
            .efi_card
            .as_ref()
            .map(|c| c.notification_url.clone())
            .unwrap_or_default();
        Arc::new(PixAutoBillingService::new(
            client as Arc<dyn PixAutoGateway>,
            subscription_service.clone(),
            notification_url,
        ))
    });

    let password_reset_service = Arc::new(PasswordResetService::new(
        Arc::new(PgPasswordResetRepository::new(pool.clone())),
    ));
    let plan_service = Arc::new(PlanService::new(
        Arc::new(PgPlanRepository::new(pool.clone())),
    ));

    AppState::new(
        pool,
        config,
        product_service,
        auth_service,
        company_service,
        customer_service,
        business_hours_service,
        category_service,
        job_role_service,
        subcategory_service,
        order_service,
        customer_address_service,
        addon_group_service,
        addon_service,
        banner_service,
        coupon_service,
        cash_service,
        finance_category_service,
        finance_service,
        wallet_service,
        subscription_service,
        payment_method_service,
        payment_service,
        card_billing,
        pix_auto,
        Arc::new(letaf_server::card_session::CardSessionStore::new()),
        password_reset_service,
        plan_service,
    )
}

/// Constrói o `EfiClient` (API PIX, mTLS) se houver config Efi válida;
/// loga e retorna `None` em caso de falha (`.p12` ausente, etc.) para
/// não derrubar o boot do server (§11). Reutilizado pelo PIX imediato
/// e pelo Pix Automático.
fn build_efi_client(config: &AppConfig) -> Option<Arc<EfiClient>> {
    let efi_cfg = config.efi.clone()?;
    match EfiClient::new(efi_cfg.clone()) {
        Ok(client) => {
            tracing::info!(
                "Efi (PIX) habilitada · env={} · base={}",
                efi_cfg.env,
                efi_cfg.base_url()
            );
            Some(Arc::new(client))
        }
        Err(e) => {
            tracing::warn!("Efi desabilitada (falha ao iniciar cliente): {e}");
            None
        }
    }
}

/// Constrói o `CardBillingService` (cartão recorrente via API Cobranças
/// da Efi) quando `efi_card` está configurado. `None` desabilita os
/// endpoints de cartão sem derrubar o server (§11).
fn build_card_billing(
    config: &AppConfig,
    subscriptions: Arc<SubscriptionService>,
) -> Option<Arc<CardBillingService>> {
    let cfg: EfiCardConfig = config.efi_card.clone()?;
    match EfiCardClient::new(cfg.clone()) {
        Ok(client) => {
            tracing::info!(
                "Efi Cobranças (cartão) habilitada · env={} · base={}",
                cfg.env,
                cfg.base_url()
            );
            let gateway: Arc<dyn CardGateway> = Arc::new(client);
            Some(Arc::new(CardBillingService::new(
                gateway,
                subscriptions,
                cfg.notification_url,
            )))
        }
        Err(e) => {
            tracing::warn!("Efi Cobranças desabilitada (falha ao iniciar cliente): {e}");
            None
        }
    }
}

/// Inicia o servidor HTTP axum no endereço configurado.
async fn serve(state: AppState, config: &AppConfig) {
    let app = Router::new()
        .merge(routes::create_routes())
        .layer(build_cors_layer(config))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", config.server_port);
    tracing::info!("Server running on {addr}");

    let listener = TcpListener::bind(&addr).await.expect("Failed to bind");
    axum::serve(listener, app).await.expect("Server failed");
}

/// Constrói o CorsLayer a partir da configuração.
///
/// Regras aplicadas (AI_RULES.md §8, §11):
/// - Funções pequenas com responsabilidade única
/// - Avisa quando CORS_ORIGINS=* (inseguro em produção)
fn build_cors_layer(config: &AppConfig) -> CorsLayer {
    if config.cors_origins.contains(&"*".to_string()) {
        tracing::warn!("CORS_ORIGINS=* — qualquer origem aceita. NÃO use em produção.");
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any)
    } else {
        let origins: Vec<_> = config
            .cors_origins
            .iter()
            .filter_map(|o| o.parse().ok())
            .collect();
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(origins))
            .allow_methods(Any)
            .allow_headers(Any)
    }
}
