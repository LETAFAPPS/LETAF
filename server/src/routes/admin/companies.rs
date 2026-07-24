//! Painel do super admin — empresas (tenants): cadastro, detalhe,
//! bloqueio de acesso e exclusão lógica.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use letaf_core::auth::model::UserRole;
use letaf_core::error::CoreError;

use crate::context::AppState;
use crate::error::ServerError;
use crate::middleware::auth::AuthClaims;

use super::{audit, brl, email_available, require_super_admin, tenants, EMAIL_TAKEN, PLATFORM_SUBDOMAIN};
// ── Empresas (tenants) ───────────────────────────────────────────────────
#[derive(Serialize)]
pub(super) struct CompanyRow {
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
pub(super) struct CreateCompanyRequest {
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

pub(super) async fn create_company(
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

// ── Detalhe da empresa (central de suporte) ──────────────────────────────
#[derive(Serialize)]
pub(super) struct CompanyDetail {
    id: Uuid,
    name: String,
    subdomain: String,
    created_at: String,
    active: bool,
    // Cadastro
    document: String,
    phone: String,
    whatsapp: String,
    email: String,
    address: String,
    city_uf: String,
    // Assinatura corrente
    plan: String,
    plan_amount: String,
    status: String,
    next_charge: String,
    discount: String,
    payment_method: String,
    // Faturas
    invoices_total: usize,
    invoices_pending: usize,
    // Uso operacional (diagnóstico de suporte)
    orders_count: i64,
    products_count: i64,
    customers_count: i64,
    /// Data do pedido mais recente ("" se nunca vendeu).
    last_order_at: String,
}

/// Consolida cadastro + assinatura + faturas de um tenant numa só resposta
/// (evita 3 round-trips do painel). Somente leitura.
pub(super) async fn company_detail(
    State(state): State<AppState>,
    auth: AuthClaims,
    Path(id): Path<Uuid>,
) -> Result<Json<CompanyDetail>, ServerError> {
    require_super_admin(&auth)?;
    let c = state
        .company_service
        .find_by_id(id)
        .await?
        .ok_or_else(|| ServerError::Core(CoreError::NotFound("Empresa não encontrada".into())))?;

    let sub = state.subscription_service.find_current(id).await?;
    let (plan, plan_amount, status, next_charge, discount, payment_method) = match &sub {
        Some(s) => {
            let terms = state.subscription_service.terms(s);
            (
                terms.name.clone(),
                brl(terms.amount),
                s.status.as_str().to_string(),
                s.next_charge_date
                    .map(|d| d.format("%d/%m/%Y").to_string())
                    .unwrap_or_default(),
                brl(s.plan_discount_monthly),
                s.payment_method.label.clone(),
            )
        }
        None => (
            String::new(),
            String::new(),
            "none".into(),
            String::new(),
            brl(Decimal::ZERO),
            String::new(),
        ),
    };

    let invoices = state.subscription_service.find_invoices(id).await?;
    let invoices_pending = invoices
        .iter()
        .filter(|i| i.status.as_str() != "paid")
        .count();

    // Uso operacional — contagens por query dedicada (§13) e só o último
    // pedido (1 linha), nunca a lista inteira.
    let orders_count = state.order_service.count_all(id).await.unwrap_or(0);
    let products_count = state.product_service.count_all(id).await.unwrap_or(0);
    let customers_count = state.customer_service.count_all(id).await.unwrap_or(0);
    let last_order_at = state
        .order_service
        .find_all_paged(id, 1, 0)
        .await
        .ok()
        .and_then(|v| v.into_iter().next())
        .map(|o| o.base.created_at.format("%d/%m/%Y %H:%M").to_string())
        .unwrap_or_default();

    let city_uf = match (c.city.as_deref(), c.uf.as_deref()) {
        (Some(city), Some(uf)) if !city.is_empty() && !uf.is_empty() => format!("{city}/{uf}"),
        (Some(city), _) => city.to_string(),
        (_, Some(uf)) => uf.to_string(),
        _ => String::new(),
    };

    Ok(Json(CompanyDetail {
        id: c.id,
        name: c.name,
        subdomain: c.subdomain,
        created_at: c.created_at.format("%d/%m/%Y").to_string(),
        active: c.active,
        document: c.document.unwrap_or_default(),
        phone: c.phone.unwrap_or_default(),
        whatsapp: c.whatsapp.unwrap_or_default(),
        email: c.email.unwrap_or_default(),
        address: c.address.unwrap_or_default(),
        city_uf,
        plan,
        plan_amount,
        status,
        next_charge,
        discount,
        payment_method,
        invoices_total: invoices.len(),
        invoices_pending,
        orders_count,
        products_count,
        customers_count,
        last_order_at,
    }))
}

/// Exclusão LÓGICA (soft delete) de uma empresa (tenant) pelo super admin.
/// Não remove fisicamente (§6): marca `deleted_at`. O login do tenant deixa
/// de resolver a empresa (find_by_subdomain filtra `deleted_at IS NULL`).
pub(super) async fn delete_company(
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
pub(super) struct SetActiveRequest {
    active: bool,
}

pub(super) async fn set_company_active(
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

pub(super) async fn list_companies(
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

// ── Últimos pedidos de uma empresa (diagnóstico de suporte) ──────────────
#[derive(Serialize)]
pub(super) struct CompanyOrderRow {
    number: i64,
    status: String,
    total: String,
    at: String,
}

/// Os 10 pedidos mais recentes do tenant — só para o suporte enxergar se
/// as vendas estão entrando. Somente leitura, sem itens (§13: página de 10,
/// nunca a lista inteira).
pub(super) async fn list_company_orders(
    State(state): State<AppState>,
    auth: AuthClaims,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<CompanyOrderRow>>, ServerError> {
    require_super_admin(&auth)?;
    let orders = state.order_service.find_all_paged(id, 10, 0).await?;
    Ok(Json(
        orders
            .into_iter()
            .map(|o| CompanyOrderRow {
                number: o.number,
                status: o.status.to_string(),
                total: brl(o.total),
                at: o.base.created_at.format("%d/%m/%Y %H:%M").to_string(),
            })
            .collect(),
    ))
}

