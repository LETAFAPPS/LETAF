//! Painel do super admin — visão geral (KPIs cross-tenant).

use axum::extract::State;
use axum::Json;
use rust_decimal::Decimal;

use crate::context::AppState;
use crate::error::ServerError;
use crate::middleware::auth::AuthClaims;

use super::{brl, require_super_admin, tenants};
use serde::Serialize;
use uuid::Uuid;
// ── Painel (visão geral) ─────────────────────────────────────────────────
#[derive(Serialize)]
pub(super) struct OverviewResponse {
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


pub(super) async fn overview(
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

