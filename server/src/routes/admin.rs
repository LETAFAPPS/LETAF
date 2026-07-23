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

use axum::extract::{Path, State};
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use axum::http::StatusCode;
use axum::routing::{delete, get, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use letaf_core::auth::model::UserRole;
use letaf_core::company::model::Company;
use letaf_core::error::CoreError;
use letaf_core::plan::model::Plan;
use letaf_core::plan::service::PlanInput;
use letaf_core::subscription::model::{PlanKind, SubscriptionStatus};

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

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/admin/overview", get(overview))
        .route("/admin/companies", get(list_companies).post(create_company))
        .route("/admin/companies/{id}", delete(delete_company))
        .route("/admin/companies/{id}/active", put(set_company_active))
        .route("/admin/subscriptions", get(list_subscriptions))
        .route("/admin/subscriptions/{company_id}", put(update_subscription))
        .route("/admin/companies/{id}/invoices", get(list_invoices))
        .route(
            "/admin/companies/{id}/invoices/{invoice_id}/paid",
            put(mark_invoice_paid),
        )
        .route("/admin/admins", get(list_admins).post(create_admin))
        .route("/admin/admins/{id}", put(update_admin).delete(delete_admin))
        .route("/admin/plans", get(list_plans).post(create_plan))
        .route("/admin/plans/{id}", put(update_plan).delete(delete_plan))
        .route("/admin/audit", get(list_audit))
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

// ── Painel (visão geral) ─────────────────────────────────────────────────
#[derive(Serialize)]
struct OverviewResponse {
    companies: usize,
    active_subscriptions: usize,
    overdue_subscriptions: usize,
    cancelled_subscriptions: usize,
    super_admins: usize,
    /// Empresas (tenants) criadas no mês corrente.
    new_companies_month: usize,
    /// Receita mensal recorrente (MRR) das assinaturas ATIVAS, já em
    /// pt-BR ("R$ 1.234,56"). Normaliza cada ciclo para o valor por mês.
    mrr: String,
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

async fn overview(
    State(state): State<AppState>,
    auth: AuthClaims,
) -> Result<Json<OverviewResponse>, ServerError> {
    require_super_admin(&auth)?;
    let tenants = tenants(&state).await?;
    let ids: Vec<Uuid> = tenants.iter().map(|c| c.id).collect();
    let subs = state.subscription_service.find_current_for_companies(&ids).await?;

    let mut active = 0usize;
    let mut overdue = 0usize;
    let mut cancelled = 0usize;
    let mut mrr = Decimal::ZERO;
    for s in &subs {
        match s.status.as_str() {
            "active" => {
                active += 1;
                // Valor líquido do ciclo ÷ meses do ciclo = valor/mês.
                let terms = state.subscription_service.terms(s);
                mrr += terms.amount / Decimal::from(terms.months.max(1));
            }
            "overdue" => overdue += 1,
            "cancelled" => cancelled += 1,
            _ => {}
        }
    }

    // Novas empresas no mês corrente.
    let now = chrono::Utc::now().naive_utc();
    let new_companies_month = tenants
        .iter()
        .filter(|c| {
            c.created_at.format("%Y-%m").to_string() == now.format("%Y-%m").to_string()
        })
        .count();

    let admins = state.auth_service.find_all(auth.0.company_id).await?;
    Ok(Json(OverviewResponse {
        companies: tenants.len(),
        active_subscriptions: active,
        overdue_subscriptions: overdue,
        cancelled_subscriptions: cancelled,
        super_admins: admins.len(),
        new_companies_month,
        mrr: brl(mrr),
    }))
}

// ── Empresas (tenants) ───────────────────────────────────────────────────
#[derive(Serialize)]
struct CompanyRow {
    id: Uuid,
    name: String,
    subdomain: String,
    created_at: String,
    plan: String,
    status: String,
    /// Acesso do tenant: `true` = ativa, `false` = suspensa.
    active: bool,
}

/// Cadastro de um novo estabelecimento (tenant) + seu administrador
/// inicial. Sem admin a empresa não teria como logar, então os dois são
/// criados juntos (§11 — company_id do novo tenant é gerado no domínio,
/// nunca vindo do frontend).
#[derive(Deserialize)]
struct CreateCompanyRequest {
    name: String,
    subdomain: String,
    admin_name: String,
    admin_email: String,
    admin_password: String,
    // Informações do estabelecimento (opcionais no cadastro).
    #[serde(default)]
    address: Option<String>,
    #[serde(default)]
    phone: Option<String>,
    #[serde(default)]
    whatsapp: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    document: Option<String>,
    #[serde(default)]
    neighborhood: Option<String>,
    #[serde(default)]
    zip_code: Option<String>,
    #[serde(default)]
    city: Option<String>,
    #[serde(default)]
    uf: Option<String>,
    #[serde(default)]
    logo_data: Option<String>,
    #[serde(default)]
    cover_data: Option<String>,
    /// Desconto comercial em R$ por mês na mensalidade (0 = sem desconto).
    #[serde(default)]
    plan_discount: Option<f64>,
}

/// `Some("")`/só espaços → `None`; caso contrário devolve o texto aparado.
fn none_if_blank(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

async fn create_company(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(body): Json<CreateCompanyRequest>,
) -> Result<(StatusCode, Json<Value>), ServerError> {
    require_super_admin(&auth)?;
    let name = body.name.trim().to_string();
    let subdomain = body.subdomain.trim().to_lowercase();
    let admin_name = body.admin_name.trim().to_string();
    let admin_email = body.admin_email.trim().to_string();

    if name.is_empty() || subdomain.is_empty() || admin_name.is_empty() || admin_email.is_empty() {
        return Err(ServerError::Core(CoreError::Validation(
            "Preencha nome, subdomínio, nome e e-mail do administrador".into(),
        )));
    }
    if body.admin_password.trim().is_empty() {
        return Err(ServerError::Core(CoreError::Validation(
            "Defina uma senha para o administrador".into(),
        )));
    }
    if !subdomain.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(ServerError::Core(CoreError::Validation(
            "Subdomínio inválido: use apenas letras, números e hífen".into(),
        )));
    }
    // Subdomínio único (identifica o tenant nas requisições).
    if state.company_service.find_by_subdomain(&subdomain).await?.is_some() {
        return Err(ServerError::Core(CoreError::Validation(
            "Este subdomínio já está em uso".into(),
        )));
    }
    // E-mail único no sistema (login é global por e-mail).
    if !email_available(&state, &admin_email, None).await {
        return Err(ServerError::Core(CoreError::Validation(EMAIL_TAKEN.into())));
    }

    // 1) Cria o tenant. 2) Aplica as informações. 3) Cria o admin inicial.
    // Em qualquer falha depois do passo 1, desfaz a empresa (evita um tenant
    // órfão/incompleto sem quem consiga acessar).
    let company = state.company_service.create(name.clone(), subdomain.clone()).await?;

    let info = letaf_core::company::service::UpdateInfoInput {
        name,
        address: none_if_blank(body.address),
        phone: none_if_blank(body.phone),
        whatsapp: none_if_blank(body.whatsapp),
        email: none_if_blank(body.email),
        instagram: None,
        document: none_if_blank(body.document),
        neighborhood: none_if_blank(body.neighborhood),
        zip_code: none_if_blank(body.zip_code),
        city: none_if_blank(body.city),
        uf: none_if_blank(body.uf),
        logo_data: none_if_blank(body.logo_data),
        cover_data: none_if_blank(body.cover_data),
        products_per_page: 20,
        orders_per_page: 20,
    };
    if let Err(e) = state.company_service.update_info(company.id, info).await {
        let _ = state.company_service.soft_delete(company.id).await;
        return Err(ServerError::Core(e));
    }

    if let Err(e) = state
        .auth_service
        .create(company.id, admin_email, body.admin_password, admin_name, UserRole::Admin)
        .await
    {
        let _ = state.company_service.soft_delete(company.id).await;
        return Err(ServerError::Core(e));
    }

    // 4) Desconto comercial (R$/mês) na mensalidade, se informado. Garante
    //    a assinatura (seed) e grava o desconto — o billing (que usa
    //    `terms()`) passa a cobrar o valor com o abatimento. Best-effort:
    //    a empresa+admin já são válidos; erro aqui só é logado.
    let discount = rust_decimal::Decimal::from_f64(body.plan_discount.unwrap_or(0.0)).unwrap_or_default().max(rust_decimal::Decimal::ZERO);
    if discount > rust_decimal::Decimal::ZERO {
        let today = chrono::Utc::now().date_naive();
        let _ = state.subscription_service.ensure_seed(company.id, today).await;
        if let Err(e) = state
            .subscription_service
            .set_plan_discount(company.id, discount)
            .await
        {
            tracing::error!("Falha ao aplicar desconto ({discount}) na empresa {}: {e}", company.id);
        }
    }

    audit(
        &state, &auth, "company.create", "company", Some(company.id),
        format!("{} ({})", company.name, subdomain), String::new(),
    )
    .await;

    Ok((
        StatusCode::CREATED,
        Json(json!({ "id": company.id, "subdomain": subdomain })),
    ))
}

/// Exclusão LÓGICA (soft delete) de uma empresa (tenant) pelo super admin.
/// Não remove fisicamente (§6): marca `deleted_at`. O login do tenant deixa
/// de resolver a empresa (find_by_subdomain filtra `deleted_at IS NULL`).
async fn delete_company(
    State(state): State<AppState>,
    auth: AuthClaims,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ServerError> {
    require_super_admin(&auth)?;
    // Nunca deixar excluir a própria empresa-plataforma.
    let mut label = String::new();
    if let Some(c) = state.company_service.find_by_id(id).await? {
        if c.subdomain == PLATFORM_SUBDOMAIN {
            return Err(ServerError::Core(CoreError::Validation(
                "A empresa-plataforma não pode ser excluída".into(),
            )));
        }
        label = c.name;
    }
    state.company_service.soft_delete(id).await?;
    audit(&state, &auth, "company.delete", "company", Some(id), label, String::new()).await;
    Ok(Json(json!({ "ok": true })))
}

/// Suspende (active=false) ou reativa (active=true) o acesso de um tenant.
/// O bloqueio é aplicado no gate de login (§11). Não permite suspender a
/// empresa-plataforma.
#[derive(Deserialize)]
struct SetActiveRequest {
    active: bool,
}

async fn set_company_active(
    State(state): State<AppState>,
    auth: AuthClaims,
    Path(id): Path<Uuid>,
    Json(body): Json<SetActiveRequest>,
) -> Result<Json<Value>, ServerError> {
    require_super_admin(&auth)?;
    if let Some(c) = state.company_service.find_by_id(id).await? {
        if c.subdomain == PLATFORM_SUBDOMAIN {
            return Err(ServerError::Core(CoreError::Validation(
                "A empresa-plataforma não pode ser suspensa".into(),
            )));
        }
    }
    let company = state.company_service.set_active(id, body.active).await?;
    audit(
        &state, &auth,
        if body.active { "company.reactivate" } else { "company.suspend" },
        "company", Some(id), company.name,
        if body.active { "acesso liberado" } else { "acesso bloqueado" },
    )
    .await;
    Ok(Json(json!({ "ok": true })))
}

async fn list_companies(
    State(state): State<AppState>,
    auth: AuthClaims,
) -> Result<Json<Vec<CompanyRow>>, ServerError> {
    require_super_admin(&auth)?;
    let tenants = tenants(&state).await?;
    let ids: Vec<Uuid> = tenants.iter().map(|c| c.id).collect();
    let subs = state.subscription_service.find_current_for_companies(&ids).await?;
    let by_company: std::collections::HashMap<Uuid, &_> =
        subs.iter().map(|s| (s.base.company_id, s)).collect();
    let mut rows = Vec::with_capacity(tenants.len());
    for c in tenants {
        let (plan, status) = match by_company.get(&c.id) {
            Some(sub) => (sub.plan_kind.as_str().to_string(), sub.status.as_str().to_string()),
            None => (String::new(), "none".to_string()),
        };
        rows.push(CompanyRow {
            id: c.id,
            name: c.name,
            subdomain: c.subdomain,
            created_at: c.created_at.format("%d/%m/%Y").to_string(),
            plan,
            status,
            active: c.active,
        });
    }
    Ok(Json(rows))
}

// ── Assinaturas & planos ─────────────────────────────────────────────────
#[derive(Serialize)]
struct SubscriptionRow {
    company_id: Uuid,
    company_name: String,
    plan: String,
    status: String,
    next_charge: String,
    payment_kind: String,
    /// Desconto comercial em R$/mês (número puro, ex.: "10").
    discount: String,
}

async fn list_subscriptions(
    State(state): State<AppState>,
    auth: AuthClaims,
) -> Result<Json<Vec<SubscriptionRow>>, ServerError> {
    require_super_admin(&auth)?;
    let tenants = tenants(&state).await?;
    let ids: Vec<Uuid> = tenants.iter().map(|c| c.id).collect();
    let subs = state.subscription_service.find_current_for_companies(&ids).await?;
    let by_company: std::collections::HashMap<Uuid, &_> =
        subs.iter().map(|s| (s.base.company_id, s)).collect();
    let mut rows = Vec::with_capacity(tenants.len());
    for c in tenants {
        if let Some(sub) = by_company.get(&c.id) {
            rows.push(SubscriptionRow {
                company_id: c.id,
                company_name: c.name,
                plan: sub.plan_kind.as_str().to_string(),
                status: sub.status.as_str().to_string(),
                next_charge: sub
                    .next_charge_date
                    .map(|d| d.format("%d/%m/%Y").to_string())
                    .unwrap_or_default(),
                payment_kind: sub.payment_method.kind.clone(),
                discount: sub.plan_discount_monthly.normalize().to_string(),
            });
        }
    }
    Ok(Json(rows))
}

/// Gestão da assinatura de uma empresa pelo super admin. Aplica apenas os
/// campos presentes: trocar plano, mudar status e/ou ajustar o desconto
/// comercial. A autoridade é o backend (§11) — a UI só solicita.
#[derive(Deserialize)]
struct UpdateSubscriptionRequest {
    /// "monthly" | "semestral" | "annual".
    #[serde(default)]
    plan: Option<String>,
    /// "active" | "overdue" | "cancelled".
    #[serde(default)]
    status: Option<String>,
    /// Desconto comercial em R$/mês (>= 0).
    #[serde(default)]
    discount: Option<f64>,
}

async fn update_subscription(
    State(state): State<AppState>,
    auth: AuthClaims,
    Path(company_id): Path<Uuid>,
    Json(body): Json<UpdateSubscriptionRequest>,
) -> Result<StatusCode, ServerError> {
    require_super_admin(&auth)?;
    let today = chrono::Utc::now().date_naive();
    // Garante que a assinatura exista (empresas antigas podem não ter seed).
    state.subscription_service.ensure_seed(company_id, today).await?;

    // Ordem: plano → status → desconto. `change_plan` reativa a assinatura
    // e recalcula a próxima cobrança; aplicar o status depois preserva a
    // intenção (ex.: cancelar após trocar de plano).
    let mut changes: Vec<String> = Vec::new();
    if let Some(plan) = body.plan {
        state
            .subscription_service
            .change_plan(company_id, PlanKind::from_str(&plan), today)
            .await?;
        changes.push(format!("plano: {plan}"));
    }
    if let Some(status) = body.status {
        state
            .subscription_service
            .set_status(company_id, SubscriptionStatus::from_str(&status))
            .await?;
        changes.push(format!("status: {status}"));
    }
    if let Some(discount) = body.discount {
        let dec = Decimal::from_f64(discount).unwrap_or_default().max(Decimal::ZERO);
        state.subscription_service.set_plan_discount(company_id, dec).await?;
        changes.push(format!("desconto: {dec}"));
    }
    let label = state
        .company_service
        .find_by_id(company_id)
        .await?
        .map(|c| c.name)
        .unwrap_or_default();
    audit(
        &state, &auth, "subscription.update", "subscription", Some(company_id),
        label, changes.join(" · "),
    )
    .await;
    Ok(StatusCode::OK)
}

// ── Faturas de uma empresa ───────────────────────────────────────────────
#[derive(Serialize)]
struct InvoiceRow {
    id: Uuid,
    number: String,
    description: String,
    /// Valor já formatado em pt-BR ("R$ 200,00").
    amount: String,
    status: String,
    issued_at: String,
    paid_at: String,
    method: String,
}

/// Histórico de faturas do tenant (mais recentes primeiro).
async fn list_invoices(
    State(state): State<AppState>,
    auth: AuthClaims,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<InvoiceRow>>, ServerError> {
    require_super_admin(&auth)?;
    let mut invoices = state.subscription_service.find_invoices(id).await?;
    invoices.sort_by_key(|i| std::cmp::Reverse(i.issued_at));
    let rows = invoices
        .into_iter()
        .map(|i| InvoiceRow {
            id: i.base.id,
            number: i.number,
            description: i.description,
            amount: brl(i.amount),
            status: i.status.as_str().to_string(),
            issued_at: i.issued_at.format("%d/%m/%Y").to_string(),
            paid_at: i
                .paid_at
                .map(|d| d.format("%d/%m/%Y").to_string())
                .unwrap_or_default(),
            method: i.method_label,
        })
        .collect();
    Ok(Json(rows))
}

/// Baixa manual de uma fatura (ex.: pagamento fora do gateway). O service
/// é idempotente e reativa a assinatura se não restarem pendências.
async fn mark_invoice_paid(
    State(state): State<AppState>,
    auth: AuthClaims,
    Path((id, invoice_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ServerError> {
    require_super_admin(&auth)?;
    let inv = state
        .subscription_service
        .mark_invoice_paid(id, invoice_id, None)
        .await?;
    audit(
        &state, &auth, "invoice.paid", "invoice", Some(invoice_id),
        inv.number.clone(), format!("baixa manual · {}", brl(inv.amount)),
    )
    .await;
    Ok(Json(json!({ "ok": true })))
}

// ── Auditoria ────────────────────────────────────────────────────────────
#[derive(Serialize)]
struct AuditRowOut {
    actor: String,
    action: String,
    target: String,
    details: String,
    /// "DD/MM/AAAA HH:MM".
    at: String,
}

/// Trilha das últimas ações do super admin (somente leitura — §11).
async fn list_audit(
    State(state): State<AppState>,
    auth: AuthClaims,
) -> Result<Json<Vec<AuditRowOut>>, ServerError> {
    require_super_admin(&auth)?;
    let entries = state.audit_service.find_recent(200).await?;
    Ok(Json(
        entries
            .into_iter()
            .map(|e| AuditRowOut {
                actor: e.actor_name,
                action: e.action,
                target: e.target_label,
                details: e.details,
                at: e.created_at.format("%d/%m/%Y %H:%M").to_string(),
            })
            .collect(),
    ))
}

// ── Administradores (gestão dos super admins) ────────────────────────────
#[derive(Serialize)]
struct AdminRow {
    id: Uuid,
    name: String,
    email: String,
}

async fn list_admins(
    State(state): State<AppState>,
    auth: AuthClaims,
) -> Result<Json<Vec<AdminRow>>, ServerError> {
    require_super_admin(&auth)?;
    let users = state.auth_service.find_all(auth.0.company_id).await?;
    let rows = users
        .into_iter()
        .filter(|u| u.role.is_super_admin())
        .map(|u| AdminRow {
            id: u.base.id,
            name: u.name,
            email: u.email,
        })
        .collect();
    Ok(Json(rows))
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

#[derive(Deserialize)]
struct CreateAdminRequest {
    name: String,
    email: String,
    password: String,
}

async fn create_admin(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(body): Json<CreateAdminRequest>,
) -> Result<(StatusCode, Json<Value>), ServerError> {
    require_super_admin(&auth)?;
    if !email_available(&state, &body.email, None).await {
        return Err(ServerError::Core(CoreError::Validation(EMAIL_TAKEN.into())));
    }
    let user = state
        .auth_service
        .create(
            auth.0.company_id,
            body.email,
            body.password,
            body.name,
            UserRole::SuperAdmin,
        )
        .await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": user.base.id }))))
}

#[derive(Deserialize)]
struct UpdateAdminRequest {
    name: String,
    email: String,
    /// Nova senha; vazio/ausente mantém a atual.
    #[serde(default)]
    password: Option<String>,
}

async fn update_admin(
    State(state): State<AppState>,
    auth: AuthClaims,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateAdminRequest>,
) -> Result<Json<Value>, ServerError> {
    require_super_admin(&auth)?;
    if !email_available(&state, &body.email, Some(id)).await {
        return Err(ServerError::Core(CoreError::Validation(EMAIL_TAKEN.into())));
    }
    state
        .auth_service
        // Painel do super admin não mexe na foto do operador → None.
        .update_credentials(auth.0.company_id, id, body.email, body.name, body.password, None)
        .await?;
    Ok(Json(json!({ "ok": true })))
}

async fn delete_admin(
    State(state): State<AppState>,
    auth: AuthClaims,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ServerError> {
    require_super_admin(&auth)?;
    // Não pode remover a si mesmo.
    if id == auth.0.sub {
        return Err(ServerError::Core(CoreError::Validation(
            "Você não pode remover o próprio usuário.".into(),
        )));
    }
    // Não pode remover o último super admin (não deixar a plataforma sem acesso).
    let admins = state.auth_service.find_all(auth.0.company_id).await?;
    let count = admins.iter().filter(|u| u.role.is_super_admin()).count();
    if count <= 1 {
        return Err(ServerError::Core(CoreError::Validation(
            "Deve existir ao menos um administrador.".into(),
        )));
    }
    state.auth_service.soft_delete(auth.0.company_id, id).await?;
    Ok(Json(json!({ "ok": true })))
}

// ── Catálogo de planos (CRUD do super admin) ─────────────────────────────
/// Payload de plano (reusado pela vitrine das lojas em subscriptions.rs).
#[derive(Serialize)]
pub(crate) struct PlanPayload {
    pub id: Uuid,
    pub name: String,
    pub amount: f64,
    pub period_months: i32,
    pub trial_days: i32,
    pub description: String,
    pub highlight_label: String,
    pub active: bool,
    pub sort_order: i32,
    /// Mensalidade efetiva (R$/mês) — conveniência para a UI.
    pub monthly_price: f64,
}

pub(crate) fn plan_payload(p: Plan) -> PlanPayload {
    let monthly_price = p.monthly_price().to_f64().unwrap_or(0.0);
    PlanPayload {
        id: p.id,
        name: p.name,
        amount: p.amount.to_f64().unwrap_or(0.0),
        period_months: p.period_months,
        trial_days: p.trial_days,
        description: p.description,
        highlight_label: p.highlight_label,
        active: p.active,
        sort_order: p.sort_order,
        monthly_price,
    }
}

/// Plano + quantas empresas o usam. `flatten` preserva o formato de
/// `PlanPayload` (que também serve a vitrine das lojas) e só acrescenta a
/// contagem, exclusiva do painel.
#[derive(Serialize)]
struct AdminPlanPayload {
    #[serde(flatten)]
    plan: PlanPayload,
    companies: usize,
}

/// Quantas empresas usam cada plano do catálogo (assinatura corrente).
async fn plan_usage(state: &AppState) -> Result<std::collections::HashMap<Uuid, usize>, ServerError> {
    let tenants = tenants(state).await?;
    let ids: Vec<Uuid> = tenants.iter().map(|c| c.id).collect();
    let subs = state.subscription_service.find_current_for_companies(&ids).await?;
    let mut usage: std::collections::HashMap<Uuid, usize> = std::collections::HashMap::new();
    for s in subs {
        if let Some(plan_id) = s.plan_id {
            *usage.entry(plan_id).or_insert(0) += 1;
        }
    }
    Ok(usage)
}

async fn list_plans(
    State(state): State<AppState>,
    auth: AuthClaims,
) -> Result<Json<Vec<AdminPlanPayload>>, ServerError> {
    require_super_admin(&auth)?;
    let plans = state.plan_service.find_all().await?;
    let usage = plan_usage(&state).await?;
    Ok(Json(
        plans
            .into_iter()
            .map(|p| {
                let companies = usage.get(&p.id).copied().unwrap_or(0);
                AdminPlanPayload { plan: plan_payload(p), companies }
            })
            .collect(),
    ))
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
struct PlanBody {
    name: String,
    amount: Decimal,
    period_months: i32,
    #[serde(default)]
    trial_days: i32,
    #[serde(default)]
    description: String,
    #[serde(default)]
    highlight_label: String,
    #[serde(default = "default_true")]
    active: bool,
    #[serde(default)]
    sort_order: i32,
}

impl PlanBody {
    fn into_input(self) -> PlanInput {
        PlanInput {
            name: self.name,
            amount: self.amount,
            period_months: self.period_months,
            trial_days: self.trial_days,
            description: self.description,
            highlight_label: self.highlight_label,
            active: self.active,
            sort_order: self.sort_order,
        }
    }
}

async fn create_plan(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(body): Json<PlanBody>,
) -> Result<(StatusCode, Json<Value>), ServerError> {
    require_super_admin(&auth)?;
    let plan = state.plan_service.create(body.into_input()).await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": plan.id }))))
}

async fn update_plan(
    State(state): State<AppState>,
    auth: AuthClaims,
    Path(id): Path<Uuid>,
    Json(body): Json<PlanBody>,
) -> Result<Json<Value>, ServerError> {
    require_super_admin(&auth)?;
    state.plan_service.update(id, body.into_input()).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn delete_plan(
    State(state): State<AppState>,
    auth: AuthClaims,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ServerError> {
    require_super_admin(&auth)?;
    // Não excluir plano em uso: as assinaturas guardam o snapshot dos
    // termos, mas perder o plano do catálogo quebraria a gestão (§11 — a
    // autoridade é o backend, não a UI).
    let in_use = plan_usage(&state).await?.get(&id).copied().unwrap_or(0);
    if in_use > 0 {
        return Err(ServerError::Core(CoreError::Validation(format!(
            "Plano em uso por {in_use} empresa(s). Migre-as para outro plano antes de excluir."
        ))));
    }
    state.plan_service.soft_delete(id).await?;
    audit(&state, &auth, "plan.delete", "plan", Some(id), String::new(), String::new()).await;
    Ok(Json(json!({ "ok": true })))
}

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
