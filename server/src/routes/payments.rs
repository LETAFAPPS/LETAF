use axum::extract::{Path, State};
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
    amount: f64,
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
    let charge = svc
        .create_pix_charge(tenant.company_id, body.invoice_id, body.amount, &body.description)
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
    Ok(Json(ChargeView { charge }))
}
