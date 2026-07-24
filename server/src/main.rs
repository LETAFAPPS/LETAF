use std::time::Duration;

use axum::Router;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tokio::net::TcpListener;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};


use letaf_server::config::AppConfig;
use letaf_server::bootstrap::build_state;
use letaf_server::context::AppState;
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




/// Inicia o servidor HTTP axum no endereço configurado.
async fn serve(state: AppState, config: &AppConfig) {
    let app = Router::new()
        .merge(routes::create_routes())
        .layer(build_cors_layer(config))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", config.server_port);
    tracing::info!("Server running on {addr}");

    let listener = TcpListener::bind(&addr).await.expect("Failed to bind");
    // `with_connect_info` expõe o IP do socket (ConnectInfo) para o rate
    // limiter de autenticação (§11) quando não há proxy confiável.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await
    .expect("Server failed");
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
