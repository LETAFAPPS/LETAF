use axum::extract::{Query, State};
use rust_decimal::prelude::ToPrimitive;
use axum::http::StatusCode;
use axum::response::Html;
use axum::{routing::get, routing::post, Json, Router};
use chrono::Local;
use serde::{Deserialize, Serialize};

use letaf_core::error::CoreError;
use letaf_core::payment_gateway::card::{CardBillingAddress, CardCustomer};
use letaf_core::subscription::model::{Invoice, PlanKind, Subscription};

use crate::card_session::CardSessionStatus;
use crate::context::AppState;
use crate::error::ServerError;
use crate::jwt::ROLES_OPERATORS;
use crate::middleware::auth::AuthClaims;
use crate::middleware::tenant::TenantContext;

/// Rotas REST de Assinatura.
///
/// Regras aplicadas (AI_RULES.md §1, §11, §12):
/// - Apenas conversão HTTP ↔ domínio; lógica vive no service.
/// - JWT obrigatório + isolamento por `company_id` (TenantContext).
/// - `change_plan` por enquanto não cobra (gateway entra depois).
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/subscription", get(get_current))
        // Vitrine: planos ATIVOS do catálogo (cadastrados pelo super admin).
        .route("/subscription/plans", get(list_active_plans))
        .route("/subscription/plan", axum::routing::put(change_plan))
        .route("/subscription/invoices", get(list_invoices))
        // Cartão recorrente (API Cobranças da Efi). A tokenização é
        // client-side (Efi.js) numa página hosted — o cadastro do cartão
        // não passa pelo app/server.
        .route("/subscription/card", axum::routing::delete(cancel_card))
        .route("/subscription/card/status", get(card_status))
        // Abre a sessão (app, JWT) e consulta o status do cadastro.
        .route("/subscription/card/session", post(create_card_session).get(card_session_status))
        // Página pública de tokenização (Efi.js) + recebimento do token.
        .route("/pay/card", get(card_payment_page))
        .route("/pay/card/submit", post(card_payment_submit))
        // Pix Automático (mandato de débito recorrente — API PIX).
        .route(
            "/subscription/pix-auto",
            post(activate_pix_auto).delete(cancel_pix_auto),
        )
        .route("/subscription/pix-auto/status", get(pix_auto_status))
        // Webhooks públicos da Efi (sem JWT):
        // - `/webhooks/efi`     → Cobranças (cartão), token opaco.
        // - `/webhooks/efi/pix` → API PIX (Pix Automático), payload mTLS.
        .route("/webhooks/efi", post(efi_webhook))
        .route("/webhooks/efi/pix", post(efi_pix_webhook))
}

#[derive(Serialize)]
struct SubscriptionView {
    subscription: Option<Subscription>,
}

/// GET /subscription/plans — planos ativos do catálogo (qualquer operador
/// autenticado). Reusa o payload do painel super admin.
async fn list_active_plans(
    State(state): State<AppState>,
    auth: AuthClaims,
) -> Result<Json<Vec<crate::routes::admin::PlanPayload>>, ServerError> {
    auth.verify_any_role(&[
        crate::jwt::ROLE_ADMIN,
        crate::jwt::ROLE_EMPLOYEE,
        crate::jwt::ROLE_SUPER_ADMIN,
    ])?;
    let plans = state.plan_service.find_active().await?;
    Ok(Json(
        plans.into_iter().map(crate::routes::admin::plan_payload).collect(),
    ))
}

#[derive(Deserialize)]
struct ChangePlanRequest {
    /// Plano fixo legado ("monthly"/"semestral"/"annual"). Ignorado quando
    /// `plan_id` (plano do catálogo) é informado.
    #[serde(default)]
    plan: String,
    /// Plano do catálogo (super admin). Presente → assina o plano do
    /// catálogo (snapshot + trial). Ausente → troca legada por `plan`.
    #[serde(default)]
    plan_id: Option<uuid::Uuid>,
}

async fn get_current(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
) -> Result<Json<SubscriptionView>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("subscription.view")?;
    let subscription = state
        .subscription_service
        .find_current(tenant.company_id)
        .await?;
    Ok(Json(SubscriptionView { subscription }))
}

async fn change_plan(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Json(body): Json<ChangePlanRequest>,
) -> Result<Json<Subscription>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("subscription.edit")?;
    let today = Local::now().date_naive();
    // Plano do catálogo (super admin) → assina com snapshot + trial.
    let updated = if let Some(plan_id) = body.plan_id {
        let plan = state
            .plan_service
            .find_by_id(plan_id)
            .await?
            .ok_or_else(|| ServerError::Core(CoreError::NotFound("Plano não encontrado".into())))?;
        if !plan.active {
            return Err(ServerError::Core(CoreError::Validation(
                "Plano indisponível".into(),
            )));
        }
        state
            .subscription_service
            .subscribe_to_plan(tenant.company_id, &plan, today)
            .await?
    } else {
        // Legado: plano fixo por PlanKind.
        let plan = PlanKind::from_str(&body.plan);
        state
            .subscription_service
            .change_plan(tenant.company_id, plan, today)
            .await?
    };
    Ok(Json(updated))
}

async fn list_invoices(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
) -> Result<Json<Vec<Invoice>>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("subscription.view")?;
    let items = state
        .subscription_service
        .find_invoices(tenant.company_id)
        .await?;
    Ok(Json(items))
}

// ── Cartão recorrente (página hosted Efi.js) ─────────────────────
//
// Regras aplicadas (AI_RULES.md §1, §11):
// - A tokenização do cartão é client-side (Efi.js) — a Efi descontinuou
//   a tokenização server-side. O PAN/CVV NUNCA passam pelo nosso server.
// - Fluxo: app abre sessão (JWT) → navegador abre `/pay/card` → Efi.js
//   tokeniza → POST `/pay/card/submit` (token, não o cartão) → server
//   cria a assinatura. App faz polling do status da sessão.

#[derive(Serialize)]
struct CardSessionView {
    session_token: String,
}

/// Abre uma sessão de cadastro de cartão para a empresa autenticada.
async fn create_card_session(
    State(state): State<AppState>,
    auth: AuthClaims,
) -> Result<Json<CardSessionView>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    auth.require_permission("subscription.edit")?;
    // Exige o gateway de cartão configurado (payee_code), senão a página
    // não consegue tokenizar.
    if state.card_billing.is_none() || state.config.efi_card.is_none() {
        return Err(ServerError::ServiceUnavailable("Gateway de cartão não configurado"));
    }
    let token = state.card_sessions.create(auth.0.company_id);
    Ok(Json(CardSessionView { session_token: token }))
}

#[derive(Deserialize)]
struct SessionQuery {
    s: String,
}

#[derive(Serialize)]
struct CardSessionStatusView {
    /// "pending" | "completed" | "failed"
    status: String,
    error: Option<String>,
}

/// Polling do status da sessão (app, JWT).
async fn card_session_status(
    State(state): State<AppState>,
    auth: AuthClaims,
    Query(q): Query<SessionQuery>,
) -> Result<Json<CardSessionStatusView>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    auth.require_permission("subscription.view")?;
    let view = match state.card_sessions.status(&q.s, auth.0.company_id) {
        Some(CardSessionStatus::Pending) => CardSessionStatusView {
            status: "pending".into(),
            error: None,
        },
        Some(CardSessionStatus::Completed) => CardSessionStatusView {
            status: "completed".into(),
            error: None,
        },
        Some(CardSessionStatus::Failed(e)) => CardSessionStatusView {
            status: "failed".into(),
            error: Some(e),
        },
        None => CardSessionStatusView {
            status: "expired".into(),
            error: None,
        },
    };
    Ok(Json(view))
}

/// Página pública de tokenização (Efi.js). Valida a sessão e renderiza
/// o formulário com o `payee_code` da conta + plano/valor.
async fn card_payment_page(
    State(state): State<AppState>,
    Query(q): Query<SessionQuery>,
) -> Html<String> {
    let Some(company_id) = state.card_sessions.company_of(&q.s) else {
        return Html(crate::card_page::error_page("Sessão inválida ou expirada."));
    };
    let Some(cfg) = state.config.efi_card.as_ref() else {
        return Html(crate::card_page::error_page("Gateway de cartão não configurado."));
    };
    // Plano + valor para exibir.
    let (plan_label, amount) = match state.subscription_service.find_current(company_id).await {
        Ok(Some(sub)) => {
            let p = state.subscription_service.plan_for(sub.plan_kind);
            (p.label, p.total_per_charge.to_f64().unwrap_or(0.0))
        }
        _ => ("Mensal".to_string(), 0.0),
    };
    Html(crate::card_page::render(
        cfg.base_url(),
        &cfg.payee_code,
        &q.s,
        &plan_label,
        amount,
    ))
}

/// Recebe o `payment_token` (gerado no navegador) + dados do titular e
/// cria a assinatura. Público: validado pelo token de sessão no corpo.
#[derive(Deserialize)]
struct CardSubmit {
    session_token: String,
    payment_token: String,
    #[serde(default)]
    brand: String,
    last4: String,
    expiry: String,
    name: String,
    cpf: String,
    email: String,
    phone: String,
    birth: String,
    cep: String,
    street: String,
    number: String,
    neighborhood: String,
    city: String,
    state: String,
}

#[derive(Serialize)]
struct CardSubmitResult {
    ok: bool,
    error: Option<String>,
}

async fn card_payment_submit(
    State(state): State<AppState>,
    Json(b): Json<CardSubmit>,
) -> Json<CardSubmitResult> {
    let Some(company_id) = state.card_sessions.company_of(&b.session_token) else {
        return Json(CardSubmitResult {
            ok: false,
            error: Some("Sessão inválida ou expirada".into()),
        });
    };
    let Some(svc) = state.card_billing.as_ref() else {
        return Json(CardSubmitResult {
            ok: false,
            error: Some("Gateway de cartão não configurado".into()),
        });
    };
    let customer = CardCustomer {
        name: b.name,
        cpf: b.cpf,
        email: b.email,
        phone: b.phone,
        birth: b.birth,
    };
    let billing = CardBillingAddress {
        street: b.street,
        number: b.number,
        neighborhood: b.neighborhood,
        zipcode: b.cep,
        city: b.city,
        state: b.state,
    };
    match svc
        .subscribe_with_token(
            company_id,
            b.payment_token,
            b.brand,
            b.last4,
            b.expiry,
            customer,
            billing,
        )
        .await
    {
        Ok(_) => {
            state
                .card_sessions
                .set_status(&b.session_token, CardSessionStatus::Completed);
            Json(CardSubmitResult { ok: true, error: None })
        }
        Err(e) => {
            let msg = format!("{e}");
            state
                .card_sessions
                .set_status(&b.session_token, CardSessionStatus::Failed(msg.clone()));
            Json(CardSubmitResult {
                ok: false,
                error: Some(msg),
            })
        }
    }
}

async fn cancel_card(
    State(state): State<AppState>,
    auth: AuthClaims,
) -> Result<Json<Subscription>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    auth.require_permission("subscription.edit")?;
    let svc = state
        .card_billing
        .as_ref()
        .ok_or(ServerError::ServiceUnavailable("Gateway de cartão não configurado"))?;
    let sub = svc.cancel(auth.0.company_id).await?;
    Ok(Json(sub))
}

#[derive(Serialize)]
struct CardStatusView {
    status: String,
    next_charge_date: Option<chrono::NaiveDate>,
}

async fn card_status(
    State(state): State<AppState>,
    auth: AuthClaims,
) -> Result<Json<CardStatusView>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    auth.require_permission("subscription.view")?;
    let svc = state
        .card_billing
        .as_ref()
        .ok_or(ServerError::ServiceUnavailable("Gateway de cartão não configurado"))?;
    let st = svc.refresh_status(auth.0.company_id).await?;
    Ok(Json(CardStatusView {
        status: st.status,
        next_charge_date: st.next_charge_date,
    }))
}

/// Webhook da Efi. Sem JWT: a Efi envia um token opaco que só faz
/// sentido resolvido com nossas credenciais (`fetch_notification`). O
/// corpo pode vir como `application/x-www-form-urlencoded`
/// (`notification=<token>`) ou JSON (`{"notification":"<token>"}`).
async fn efi_webhook(
    State(state): State<AppState>,
    Query(q): Query<PixWebhookQuery>,
    body: String,
) -> StatusCode {
    // Autenticação de origem (§11): paridade com o webhook PIX. Quando
    // `EFI_PIX_WEBHOOK_HMAC` está configurado, exige `?hmac=<segredo>`.
    // Opt-in para não quebrar deploys que dependem de mTLS no proxy.
    if let Some(expected) = state
        .config
        .efi
        .as_ref()
        .and_then(|e| e.pix_webhook_hmac.as_deref())
    {
        let provided = q.hmac.as_deref().unwrap_or("");
        if !ct_eq(provided.as_bytes(), expected.as_bytes()) {
            tracing::warn!("Webhook Efi (cartão) rejeitado: hmac ausente ou inválido");
            return StatusCode::UNAUTHORIZED;
        }
    }
    let Some(svc) = state.card_billing.as_ref() else {
        // Sem gateway configurado, nada a processar.
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let Some(token) = extract_notification_token(&body) else {
        tracing::warn!("Webhook Efi (cartão) sem token de notificação");
        // 200 para a Efi não reenviar um payload que nunca vamos processar.
        return StatusCode::OK;
    };
    let today = Local::now().date_naive();
    match svc.apply_notification(&token, today).await {
        Ok(n) => {
            tracing::info!("Webhook Efi: {n} evento(s) de cobrança aplicados");
            StatusCode::OK
        }
        Err(e) => {
            // 500 → a Efi reenvia (backoff). Erro transitório de rede/API.
            tracing::warn!("Webhook Efi falhou: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

// ── Pix Automático ───────────────────────────────────────────────

#[derive(Deserialize)]
struct PixAutoActivateRequest {
    customer_name: String,
    customer_cpf: String,
}

/// Resposta da ativação: assinatura + QR de **autorização** que o
/// pagador escaneia no app do banco dele.
#[derive(Serialize)]
struct PixAutoActivateView {
    subscription: Subscription,
    rec_id: String,
    copia_cola: String,
    qr_code_b64: String,
    status: String,
}

async fn activate_pix_auto(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(body): Json<PixAutoActivateRequest>,
) -> Result<(StatusCode, Json<PixAutoActivateView>), ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    auth.require_permission("subscription.edit")?;
    let svc = state
        .pix_auto
        .as_ref()
        .ok_or(ServerError::ServiceUnavailable("Pix Automático não configurado"))?;
    let (sub, created) = svc
        .activate(auth.0.company_id, body.customer_name, body.customer_cpf)
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(PixAutoActivateView {
            subscription: sub,
            rec_id: created.rec_id,
            copia_cola: created.copia_cola,
            qr_code_b64: created.qr_code_b64,
            status: created.status,
        }),
    ))
}

/// Polling da autorização: consulta o gateway e devolve a assinatura
/// atualizada (na transição para ativo, já emite a 1ª cobrança).
async fn pix_auto_status(
    State(state): State<AppState>,
    auth: AuthClaims,
) -> Result<Json<Subscription>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    auth.require_permission("subscription.view")?;
    let svc = state
        .pix_auto
        .as_ref()
        .ok_or(ServerError::ServiceUnavailable("Pix Automático não configurado"))?;
    let today = Local::now().date_naive();
    let sub = svc.refresh(auth.0.company_id, today).await?;
    Ok(Json(sub))
}

async fn cancel_pix_auto(
    State(state): State<AppState>,
    auth: AuthClaims,
) -> Result<Json<Subscription>, ServerError> {
    auth.verify_any_role(ROLES_OPERATORS)?;
    auth.require_permission("subscription.edit")?;
    let svc = state
        .pix_auto
        .as_ref()
        .ok_or(ServerError::ServiceUnavailable("Pix Automático não configurado"))?;
    let sub = svc.cancel(auth.0.company_id).await?;
    Ok(Json(sub))
}

/// Query do webhook PIX: a Efi anexa `?hmac=<segredo>` quando o webhook
/// é registrado com HMAC (mecanismo nativo deles, alternativa ao mTLS).
#[derive(Deserialize)]
struct PixWebhookQuery {
    hmac: Option<String>,
}

/// Webhook da API PIX (Pix Automático).
///
/// AUTENTICAÇÃO DA ORIGEM (§11): quando `EFI_PIX_WEBHOOK_HMAC` está
/// configurado, exige a query `?hmac=<segredo>` que a Efi anexa — sem
/// isso, qualquer um que alcance a URL poderia forjar confirmação de
/// pagamento de assinatura. Se não configurado, mantém o comportamento
/// anterior (depende de mTLS no proxy) para não quebrar deploys atuais.
/// ⚠️ RECOMENDADO em produção: configurar o HMAC no painel Efi + no
/// `.env`, OU validar o certificado mTLS de cliente no proxy reverso.
async fn efi_pix_webhook(
    State(state): State<AppState>,
    Query(q): Query<PixWebhookQuery>,
    body: String,
) -> StatusCode {
    if let Some(expected) = state
        .config
        .efi
        .as_ref()
        .and_then(|e| e.pix_webhook_hmac.as_deref())
    {
        let provided = q.hmac.as_deref().unwrap_or("");
        if !ct_eq(provided.as_bytes(), expected.as_bytes()) {
            tracing::warn!("Webhook PIX rejeitado: hmac ausente ou inválido");
            return StatusCode::UNAUTHORIZED;
        }
    }
    let Some(svc) = state.pix_auto.as_ref() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let today = Local::now().date_naive();
    match svc.apply_webhook(&body, today).await {
        Ok(n) => {
            tracing::info!("Webhook PIX Automático: {n} débito(s) aplicados");
            StatusCode::OK
        }
        Err(e) => {
            tracing::warn!("Webhook PIX Automático falhou: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

/// Comparação em tempo constante de segredos — evita timing attack na
/// validação do `hmac` do webhook.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Extrai o token de notificação do corpo (form-urlencoded ou JSON).
fn extract_notification_token(body: &str) -> Option<String> {
    // Form: notification=<token>&code=...
    for pair in body.split('&') {
        if let Some(v) = pair.strip_prefix("notification=") {
            let decoded = v.replace('+', " ");
            if !decoded.is_empty() {
                return Some(decoded);
            }
        }
    }
    // JSON: {"notification":"<token>"}
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.get("notification")
                .and_then(|t| t.as_str())
                .map(|s| s.to_string())
        })
}
