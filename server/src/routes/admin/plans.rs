//! Painel do super admin — catálogo de planos de assinatura.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use letaf_core::error::CoreError;
use letaf_core::plan::model::Plan;
use letaf_core::plan::service::PlanInput;

use crate::context::AppState;
use crate::error::ServerError;
use crate::middleware::auth::AuthClaims;

use super::{audit, require_super_admin, tenants};
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
pub(super) struct AdminPlanPayload {
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

pub(super) async fn list_plans(
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
pub(super) struct PlanBody {
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

pub(super) async fn create_plan(
    State(state): State<AppState>,
    auth: AuthClaims,
    Json(body): Json<PlanBody>,
) -> Result<(StatusCode, Json<Value>), ServerError> {
    require_super_admin(&auth)?;
    let plan = state.plan_service.create(body.into_input()).await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": plan.id }))))
}

pub(super) async fn update_plan(
    State(state): State<AppState>,
    auth: AuthClaims,
    Path(id): Path<Uuid>,
    Json(body): Json<PlanBody>,
) -> Result<Json<Value>, ServerError> {
    require_super_admin(&auth)?;
    state.plan_service.update(id, body.into_input()).await?;
    Ok(Json(json!({ "ok": true })))
}

pub(super) async fn delete_plan(
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

