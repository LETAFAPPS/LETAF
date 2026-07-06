//! Seed de desenvolvimento — cria company + user iniciais no PostgreSQL.
//!
//! Regras aplicadas (AI_RULES.md §1, §8, §10, §11):
//! - Acesso ao banco via services/repositories (nunca SQL direto)
//! - Senha hasheada via AuthService (bcrypt)
//! - Idempotente: roda múltiplas vezes sem erro
//!
//! Uso:
//!   cargo run --bin seed
//!
//! Requer DATABASE_URL configurada (ou .env no diretório server/).

use std::sync::Arc;

use sqlx::postgres::PgPoolOptions;

use letaf_core::auth::service::AuthService;
use letaf_core::company::service::CompanyService;

use letaf_server::config::AppConfig;
use letaf_server::repository::auth::PgUserRepository;
use letaf_server::repository::company::PgCompanyRepository;

const SUBDOMAIN: &str = "demo";
const COMPANY_NAME: &str = "Empresa Demo";
const USER_EMAIL: &str = "admin@demo.com";
const USER_PASSWORD: &str = "admin123";
const USER_NAME: &str = "Administrador";

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let config = AppConfig::from_env();

    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&config.database_url)
        .await
        .expect("Failed to connect to PostgreSQL");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    let company_service = CompanyService::new(
        Arc::new(PgCompanyRepository::new(pool.clone())),
    );
    let auth_service = AuthService::new(
        Arc::new(PgUserRepository::new(pool)),
    );

    let company = seed_company(&company_service).await;
    seed_user(&auth_service, company.id).await;

    println!("\n=== Seed concluído ===");
    println!("  subdomain: {SUBDOMAIN}");
    println!("  email:     {USER_EMAIL}");
    println!("  password:  {USER_PASSWORD}");
}

/// Cria ou reutiliza empresa pelo subdomain.
async fn seed_company(service: &CompanyService) -> letaf_core::company::model::Company {
    if let Some(existing) = service
        .find_by_subdomain(SUBDOMAIN)
        .await
        .expect("Failed to query company")
    {
        println!("Company já existe: {} ({})", existing.name, existing.id);
        return existing;
    }

    let company = service
        .create(COMPANY_NAME.into(), SUBDOMAIN.into())
        .await
        .expect("Failed to create company");

    println!("Company Criada: {} ({})", company.name, company.id);
    company
}

/// Cria ou reutiliza usuário pelo email.
async fn seed_user(service: &AuthService, company_id: uuid::Uuid) {
    if let Some(existing) = service
        .find_by_email(company_id, USER_EMAIL)
        .await
        .expect("Failed to query user")
    {
        println!("User já existe: {} ({})", existing.email, existing.base.id);
        return;
    }

    let user = service
        .create(
            company_id,
            USER_EMAIL.into(),
            USER_PASSWORD.into(),
            USER_NAME.into(),
            letaf_core::auth::model::UserRole::Admin,
        )
        .await
        .expect("Failed to create user");

    println!("User Criado: {} ({})", user.email, user.base.id);
}
