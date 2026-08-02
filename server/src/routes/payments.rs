use axum::extract::{Path, State};
use rust_decimal::Decimal;
use axum::http::StatusCode;
use axum::{routing::get, routing::post, Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use letaf_core::error::CoreError;
use letaf_core::payment_gateway::model::PaymentCharge;

use crate::context::AppState;
use crate::error::ServerError;
use crate::jwt::ROLES_OPERATORS;
use crate::middleware::auth::AuthClaims;
use crate::middleware::tenant::TenantContext;

/// Endpoints de cobrança avulsa (PIX por enquanto).
///
/// Regras aplicadas (AI_RULES.md §1, §11, §12):
/// - Apenas conversão HTTP ↔ domínio; lógica vive no `PaymentService`.
/// - JWT obrigatório + isolamento por `company_id`.
/// - Quando `payment_service` é `None` (gateway não configurado),
///   responde 503 com mensagem explícita — não 500 silencioso.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/payments/pix/charge", post(create_pix_charge))
        .route("/payments/pix/charge/{id}", get(get_charge))
        .route(
            "/payments/pix/charge/{id}/refresh",
            post(refresh_charge_status),
        )
}

#[derive(Deserialize)]
struct CreatePixChargeRequest {
    #[serde(default)]
    invoice_id: Option<Uuid>,
    amount: Decimal,
    description: String,
}

#[derive(Serialize)]
struct ChargeView {
    charge: PaymentCharge,
}

async fn create_pix_charge(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Json(body): Json<CreatePixChargeRequest>,
) -> Result<(StatusCode, Json<ChargeView>), ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("orders.edit")?;
    let svc = state
        .payment_service
        .as_ref()
        .ok_or(ServerError::ServiceUnavailable("Gateway de pagamento não configurado"))?;
    // Quando a cobrança quita uma FATURA, o valor vem da fatura (fonte de
    // verdade), NUNCA do corpo (§11). Sem isto, `{"invoice_id": <real>,
    // "amount": 0.01}` gerava um QR de 1 centavo que, ao ser pago, marcava a
    // fatura como paga integralmente e reativava a assinatura. Também
    // confirma que a fatura pertence ao tenant — id de outra empresa não é
    // aceito.
    let amount = match body.invoice_id {
        Some(invoice_id) => {
            let fatura = state
                .subscription_service
                .find_invoices(tenant.company_id)
                .await?
                .into_iter()
                .find(|i| i.base.id == invoice_id)
                .ok_or_else(|| {
                    ServerError::Core(CoreError::NotFound("Fatura não encontrada".into()))
                })?;
            fatura.amount
        }
        None => body.amount,
    };
    let charge = svc
        .create_pix_charge(tenant.company_id, body.invoice_id, amount, &body.description)
        .await?;
    Ok((StatusCode::CREATED, Json(ChargeView { charge })))
}

async fn get_charge(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
) -> Result<Json<ChargeView>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("orders.view")?;
    let svc = state
        .payment_service
        .as_ref()
        .ok_or(ServerError::ServiceUnavailable("Gateway de pagamento não configurado"))?;
    let charge = svc
        .find_by_id(tenant.company_id, id)
        .await?
        .ok_or_else(|| ServerError::Core(CoreError::NotFound("Cobrança não encontrada".into())))?;
    Ok(Json(ChargeView { charge }))
}

async fn refresh_charge_status(
    State(state): State<AppState>,
    auth: AuthClaims,
    tenant: TenantContext,
    Path(id): Path<Uuid>,
) -> Result<Json<ChargeView>, ServerError> {
    auth.verify_any(tenant.company_id, ROLES_OPERATORS)?;
    auth.require_permission("orders.view")?;
    let svc = state
        .payment_service
        .as_ref()
        .ok_or(ServerError::ServiceUnavailable("Gateway de pagamento não configurado"))?;
    let charge = svc.refresh_status(tenant.company_id, id).await?;
    // A baixa da fatura é decisão do SERVIDOR (§3/§11) — o desktop só reflete
    // na tela. O tick de reconciliação cobre o caso do modal ter sido fechado.
    crate::charge_reconcile::settle_if_paid(&state, &charge).await;
    Ok(Json(ChargeView { charge }))
}
