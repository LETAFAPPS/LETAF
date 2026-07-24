//! Painel do super admin — assinaturas dos tenants e suas faturas.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use letaf_core::subscription::model::{PlanKind, SubscriptionStatus};

use crate::context::AppState;
use crate::error::ServerError;
use crate::middleware::auth::AuthClaims;

use super::{audit, brl, require_super_admin, tenants};
// ── Assinaturas & planos ─────────────────────────────────────────────────
#[derive(Serialize)]
pub(super) struct SubscriptionRow {
    company_id: Uuid,
    company_name: String,
    plan: String,
    status: String,
    next_charge: String,
    payment_kind: String,
    /// Desconto comercial em R$/mês (número puro, ex.: "10").
    discount: String,
}

pub(super) async fn list_subscriptions(
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
pub(super) struct UpdateSubscriptionRequest {
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

pub(super) async fn update_subscription(
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
pub(super) struct InvoiceRow {
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
pub(super) async fn list_invoices(
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
pub(super) async fn mark_invoice_paid(
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

