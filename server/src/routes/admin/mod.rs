//! Painel do super admin (plataforma) — rotas `/admin/*` cross-tenant.
//!
//! Regras aplicadas (AI_RULES.md §1, §10, §11):
//! - Autoridade no backend: TODA rota exige `role == super_admin` (JWT),
//!   validado por `AuthClaims::verify_role`. O frontend é burro.
//! - Exceção documentada ao isolamento por `company_id`: o super admin é
//!   cross-tenant por natureza (gestão da plataforma). Fora daqui, o
//!   isolamento por empresa continua obrigatório.
//! - Os usuários super admin vivem na "empresa-plataforma" (subdomínio
//!   reservado); assim a gestão deles reusa o `AuthService` já existente,
//!   escopado ao `company_id` do próprio super admin (vem do JWT).

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use axum::routing::{get, post, put};
use axum::Router;
use uuid::Uuid;

use letaf_core::auth::model::UserRole;
use letaf_core::company::model::Company;

use crate::context::AppState;
use crate::error::ServerError;
use crate::jwt::ROLE_SUPER_ADMIN;
use crate::middleware::auth::AuthClaims;

/// Subdomínio reservado da empresa-plataforma (container dos super admins).
/// Nunca é um tenant real — filtrado das listas de empresas.
pub const PLATFORM_SUBDOMAIN: &str = "__platform__";
const PLATFORM_COMPANY_NAME: &str = "LETAF · Plataforma";
/// E-mail default do super admin de plataforma; pode ser sobrescrito por
/// `PLATFORM_ADMIN_EMAIL`. É apenas um identificador — a proteção real é a
/// senha, que NUNCA é hardcoded (ver `ensure_platform_admin`).
const DEFAULT_ADMIN_EMAIL: &str = "admin@letaf.app";
const DEFAULT_ADMIN_NAME: &str = "Master Admin";

mod admins;
mod audit_log;
mod companies;
mod overview;
mod plans;
mod subscriptions;

pub(crate) use plans::{plan_payload, PlanPayload};


pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/admin/overview", get(overview::overview))
        .route("/admin/companies", get(companies::list_companies).post(companies::create_company))
        .route(
            "/admin/companies/{id}",
            get(companies::company_detail)
                .put(companies::update_company)
                .delete(companies::delete_company),
        )
        .route("/admin/companies/{id}/form", get(companies::company_form))
        .route(
            "/admin/companies/{id}/impersonate",
            post(companies::impersonate_company),
        )
        .route("/admin/companies/{id}/active", put(companies::set_company_active))
        .route("/admin/subscriptions", get(subscriptions::list_subscriptions))
        .route("/admin/subscriptions/{company_id}", put(subscriptions::update_subscription))
        .route("/admin/companies/{id}/orders", get(companies::list_company_orders))
        .route("/admin/companies/{id}/invoices", get(subscriptions::list_invoices))
        .route(
            "/admin/companies/{id}/invoices/{invoice_id}/paid",
            put(subscriptions::mark_invoice_paid),
        )
        .route("/admin/admins", get(admins::list_admins).post(admins::create_admin))
        .route("/admin/admins/{id}", put(admins::update_admin).delete(admins::delete_admin))
        .route("/admin/plans", get(plans::list_plans).post(plans::create_plan))
        .route("/admin/plans/{id}", put(plans::update_plan).delete(plans::delete_plan))
        .route("/admin/audit", get(audit_log::list_audit))
}

/// Guard: exige `super_admin`. `verify_role` NÃO checa `company_id`
/// (cross-tenant) — correto para o painel de plataforma.
fn require_super_admin(auth: &AuthClaims) -> Result<(), ServerError> {
    auth.verify_role(ROLE_SUPER_ADMIN)
}

/// Registra uma ação na trilha de auditoria (§11).
///
/// BEST-EFFORT de propósito: uma falha ao gravar o log NUNCA derruba a
/// operação já concluída — apenas loga o erro. O nome do ator é buscado
/// pelo `sub` do JWT (cai em "super admin" se não resolver).
async fn audit(
    state: &AppState,
    auth: &AuthClaims,
    action: &str,
    target_type: &str,
    target_id: Option<Uuid>,
    target_label: impl Into<String>,
    details: impl Into<String>,
) {
    let actor_name = state
        .auth_service
        .find_by_id(auth.0.company_id, auth.0.sub)
        .await
        .ok()
        .flatten()
        .map(|u| u.name)
        .unwrap_or_else(|| "super admin".to_string());
    if let Err(e) = state
        .audit_service
        .record(
            auth.0.sub,
            actor_name,
            action,
            target_type,
            target_id,
            target_label.into(),
            details.into(),
        )
        .await
    {
        tracing::error!("Falha ao registrar auditoria ({action}): {e}");
    }
}

/// Empresas reais (tenants) — exclui a empresa-plataforma.
async fn tenants(state: &AppState) -> Result<Vec<Company>, ServerError> {
    let all = state.company_service.find_all().await?;
    Ok(all
        .into_iter()
        .filter(|c| c.subdomain != PLATFORM_SUBDOMAIN)
        .collect())
}

/// Formata um valor em reais no padrão pt-BR: "R$ 1.234,56".
fn brl(v: Decimal) -> String {
    let s = format!("{:.2}", v.max(Decimal::ZERO).to_f64().unwrap_or(0.0));
    let (int_part, dec_part) = s.split_once('.').unwrap_or((s.as_str(), "00"));
    let digits: Vec<char> = int_part.chars().collect();
    let mut grouped = String::new();
    for (i, c) in digits.iter().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            grouped.push('.');
        }
        grouped.push(*c);
    }
    format!("R$ {grouped},{dec_part}")
}

/// `true` se o email está livre para o super admin em TODO o sistema.
/// O login do desktop é global por email → um email de super admin não pode
/// coincidir com o de nenhum usuário de outra empresa (senão o login fica
/// ambíguo e falha). `exclude` = id do próprio usuário (no update, o email
/// atual dele não conta como conflito).
async fn email_available(state: &AppState, email: &str, exclude: Option<Uuid>) -> bool {
    match state.auth_service.find_by_email_global(email).await {
        Ok(None) => true,
        Ok(Some(u)) => Some(u.base.id) == exclude,
        // Err = email em mais de uma empresa (ou erro de banco) → indisponível.
        Err(_) => false,
    }
}

const EMAIL_TAKEN: &str = "Este e-mail já está em uso em outra conta do sistema.";
// ── Bootstrap (chamado no startup do servidor) ───────────────────────────
/// Garante que a empresa-plataforma e um super admin default existam.
/// Idempotente: roda a cada boot sem duplicar.
pub async fn ensure_platform_admin(state: &AppState) {
    let company = match state.company_service.find_by_subdomain(PLATFORM_SUBDOMAIN).await {
        Ok(Some(c)) => c,
        Ok(None) => match state
            .company_service
            .create(PLATFORM_COMPANY_NAME.into(), PLATFORM_SUBDOMAIN.into())
            .await
        {
            Ok(c) => {
                tracing::info!("Empresa-plataforma criada ({})", c.id);
                c
            }
            Err(e) => {
                tracing::error!("Falha ao criar empresa-plataforma: {e}");
                return;
            }
        },
        Err(e) => {
            tracing::error!("Falha ao consultar empresa-plataforma: {e}");
            return;
        }
    };

    match state.auth_service.find_all(company.id).await {
        Ok(users) if users.iter().any(|u| u.role.is_super_admin()) => {}
        Ok(_) => {
            // §11: a senha inicial NUNCA é hardcoded no fonte. Vem de
            // `PLATFORM_ADMIN_PASSWORD` (env). Sem ela, NÃO criamos o super
            // admin (fail-closed) — melhor um painel inacessível até o
            // operador definir a senha do que uma conta com senha pública.
            let email = std::env::var("PLATFORM_ADMIN_EMAIL")
                .unwrap_or_else(|_| DEFAULT_ADMIN_EMAIL.to_string());
            match std::env::var("PLATFORM_ADMIN_PASSWORD") {
                Ok(password) if !password.trim().is_empty() => {
                    match state
                        .auth_service
                        .create(
                            company.id,
                            email.clone(),
                            password,
                            DEFAULT_ADMIN_NAME.into(),
                            UserRole::SuperAdmin,
                        )
                        .await
                    {
                        Ok(_) => tracing::info!(
                            "Super admin de plataforma criado (email={email}) a partir de PLATFORM_ADMIN_PASSWORD"
                        ),
                        Err(e) => tracing::error!("Falha ao criar super admin de plataforma: {e}"),
                    }
                }
                _ => tracing::warn!(
                    "Nenhum super admin de plataforma existe e PLATFORM_ADMIN_PASSWORD não está definida — \
                     painel /admin ficará inacessível. Defina PLATFORM_ADMIN_PASSWORD (e opcionalmente \
                     PLATFORM_ADMIN_EMAIL) no ambiente e reinicie para criar o super admin inicial."
                ),
            }
        }
        Err(e) => tracing::error!("Falha ao consultar super admins: {e}"),
    }
}

#[cfg(test)]
mod tests {
    /// Guard estrutural: TODO handler das rotas `/admin/*` precisa chamar
    /// `require_super_admin`. Um handler novo sem o gate seria um furo
    /// cross-tenant (§11) que passa despercebido em code review, então o
    /// teste lê os próprios fontes do módulo e cobra a chamada.
    ///
    /// Se um handler legitimamente não precisar do gate, adicione-o em
    /// `SEM_GATE` com a justificativa — a exceção fica explícita.
    #[test]
    fn todo_handler_admin_exige_super_admin() {
        /// Funções auxiliares (não são handlers de rota).
        const SEM_GATE: &[&str] = &[
            "audit",                 // helper de registro, chamado pelos handlers
            "tenants",               // helper de listagem interna
            "plan_usage",            // helper de contagem
            "email_available",       // helper de validação
            "ensure_platform_admin", // bootstrap no startup, sem requisição
        ];

        // Um arquivo por assunto — todos varridos.
        let fontes: &[(&str, &str)] = &[
            ("mod.rs", include_str!("mod.rs")),
            ("overview.rs", include_str!("overview.rs")),
            ("companies.rs", include_str!("companies.rs")),
            ("subscriptions.rs", include_str!("subscriptions.rs")),
            ("audit_log.rs", include_str!("audit_log.rs")),
            ("admins.rs", include_str!("admins.rs")),
            ("plans.rs", include_str!("plans.rs")),
        ];

        let mut faltando = Vec::new();
        for (arquivo, src) in fontes {
            // Corta o módulo de testes para não analisar a si mesmo.
            let src = src.split("#[cfg(test)]").next().unwrap_or(src);
            let marcas: Vec<usize> = src.match_indices("async fn ").map(|(i, _)| i).collect();
            for (idx, &start) in marcas.iter().enumerate() {
                let resto = &src[start + "async fn ".len()..];
                let nome: String = resto
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if SEM_GATE.contains(&nome.as_str()) {
                    continue;
                }
                let fim = marcas.get(idx + 1).copied().unwrap_or(src.len());
                if !src[start..fim].contains("require_super_admin") {
                    faltando.push(format!("{arquivo}::{nome}"));
                }
            }
        }
        assert!(
            faltando.is_empty(),
            "handlers /admin/* sem require_super_admin: {faltando:?}"
        );
    }
}
