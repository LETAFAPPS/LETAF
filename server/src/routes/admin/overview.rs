//! Painel do super admin — visão geral (KPIs cross-tenant).

use axum::extract::State;
use axum::Json;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::collections::HashMap;

use chrono::Datelike;

use crate::context::AppState;
use crate::error::ServerError;
use crate::middleware::auth::AuthClaims;

use letaf_core::company::model::Company;
use letaf_core::subscription::model::InvoiceStatus;

use super::{brl, tenants};
use serde::Serialize;
use uuid::Uuid;

/// Abreviações de mês em pt-BR (índice 0 = janeiro).
const MESES_ABBR: [&str; 12] = [
    "jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez",
];

// ── Painel (visão geral) ─────────────────────────────────────────────────
/// Um ponto do gráfico de receita anual (um mês).
#[derive(Serialize)]
pub(super) struct RevenuePoint {
    /// Rótulo do mês em pt-BR ("jan", "fev", ...).
    label: String,
    /// Valor do mês (numérico, para a altura da barra).
    amount: f64,
    /// Valor do mês já em pt-BR ("R$ 1.234,56").
    amount_brl: String,
}

/// Uma empresa recém-cadastrada (lista de "últimas do sistema").
#[derive(Serialize)]
pub(super) struct RecentCompany {
    name: String,
    subdomain: String,
    /// Data de cadastro em pt-BR ("25/07/2026").
    created_label: String,
}

#[derive(Serialize)]
pub(super) struct OverviewResponse {
    companies: usize,
    /// Empresas com acesso ativo × suspensas.
    active_companies: usize,
    suspended_companies: usize,
    active_subscriptions: usize,
    overdue_subscriptions: usize,
    cancelled_subscriptions: usize,
    /// Assinaturas ainda não ativadas (status "inactive").
    inactive_subscriptions: usize,
    super_admins: usize,
    /// Empresas (tenants) criadas no mês corrente.
    new_companies_month: usize,
    /// Receita mensal recorrente (MRR) das assinaturas ATIVAS, já em
    /// pt-BR ("R$ 1.234,56"). Normaliza cada ciclo para o valor por mês.
    mrr: String,
    /// Receita realizada (faturas pagas) nos últimos 12 meses, em pt-BR.
    annual_revenue: String,
    /// Ticket médio por assinatura ativa (ARPA), em pt-BR.
    arpa: String,
    /// Nome do plano mais assinado entre as assinaturas ativas ("—" se nenhum).
    top_plan: String,
    /// Indicações de novos estabelecimentos — funcionalidade FUTURA
    /// (placeholder; hoje sempre 0). Mantido no contrato para evolução.
    referrals: usize,
    /// Série do gráfico de receita anual (12 meses, do mais antigo ao atual).
    revenue_months: Vec<RevenuePoint>,
    /// Últimas empresas cadastradas (até 5, da mais recente para a mais antiga).
    recent_companies: Vec<RecentCompany>,
}

pub(super) async fn overview(
    State(state): State<AppState>,
    auth: AuthClaims,
) -> Result<Json<OverviewResponse>, ServerError> {
    auth.require_screen("overview")?;
    let tenants = tenants(&state).await?;
    let ids: Vec<Uuid> = tenants.iter().map(|c| c.id).collect();
    let subs = state.subscription_service.find_current_for_companies(&ids).await?;

    // Quebra por status + MRR + plano mais popular (assinaturas ativas).
    let mut active = 0usize;
    let mut overdue = 0usize;
    let mut cancelled = 0usize;
    let mut inactive = 0usize;
    let mut mrr = Decimal::ZERO;
    let mut plan_counts: HashMap<String, usize> = HashMap::new();
    for s in &subs {
        match s.status.as_str() {
            "active" => {
                active += 1;
                // Valor líquido do ciclo ÷ meses do ciclo = valor/mês.
                let terms = state.subscription_service.terms(s);
                mrr += terms.amount / Decimal::from(terms.months.max(1));
                *plan_counts.entry(terms.name).or_default() += 1;
            }
            "overdue" => overdue += 1,
            "cancelled" => cancelled += 1,
            "inactive" => inactive += 1,
            _ => {}
        }
    }

    // Ticket médio (ARPA) e plano mais popular.
    let arpa = if active > 0 {
        mrr / Decimal::from(active)
    } else {
        Decimal::ZERO
    };
    let top_plan = plan_counts
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .map(|(name, _)| name)
        .unwrap_or_else(|| "—".to_string());

    // Receita anual: faturas PAGAS dos últimos 12 meses, por mês.
    // Loop O(tenants) reusando o repository (§10); agregação em SQL fica como
    // otimização futura quando a base crescer (§13 — medir antes).
    let today = chrono::Utc::now().naive_utc().date();
    let mut months: Vec<(i32, u32)> = Vec::with_capacity(12);
    let (mut y, mut m) = (today.year(), today.month());
    for _ in 0..12 {
        months.push((y, m));
        if m == 1 {
            m = 12;
            y -= 1;
        } else {
            m -= 1;
        }
    }
    months.reverse();
    let month_index: HashMap<(i32, u32), usize> =
        months.iter().enumerate().map(|(i, &ym)| (ym, i)).collect();
    let mut buckets = [Decimal::ZERO; 12];
    for c in &tenants {
        let invoices = state.subscription_service.find_invoices(c.id).await.unwrap_or_default();
        for inv in invoices {
            if inv.status != InvoiceStatus::Paid {
                continue;
            }
            let Some(paid) = inv.paid_at else { continue };
            if let Some(&i) = month_index.get(&(paid.year(), paid.month())) {
                buckets[i] += inv.amount;
            }
        }
    }
    let annual: Decimal = buckets.iter().copied().sum();
    let revenue_months: Vec<RevenuePoint> = months
        .iter()
        .zip(buckets.iter())
        .map(|(&(_, mm), &amt)| RevenuePoint {
            label: MESES_ABBR[(mm - 1) as usize].to_string(),
            amount: amt.to_f64().unwrap_or(0.0),
            amount_brl: brl(amt),
        })
        .collect();

    // Empresas ativas × suspensas e novas no mês corrente.
    let active_companies = tenants.iter().filter(|c| c.active).count();
    let suspended_companies = tenants.len().saturating_sub(active_companies);
    let now = chrono::Utc::now().naive_utc();
    let new_companies_month = tenants
        .iter()
        .filter(|c| c.created_at.format("%Y-%m").to_string() == now.format("%Y-%m").to_string())
        .count();

    // Últimas empresas cadastradas (top 5 por data de criação).
    let mut recentes: Vec<&Company> = tenants.iter().collect();
    recentes.sort_by_key(|c| std::cmp::Reverse(c.created_at));
    let recent_companies: Vec<RecentCompany> = recentes
        .iter()
        .take(5)
        .map(|c| RecentCompany {
            name: c.name.clone(),
            subdomain: c.subdomain.clone(),
            created_label: c.created_at.format("%d/%m/%Y").to_string(),
        })
        .collect();

    let admins = state.auth_service.find_all(auth.0.company_id).await?;
    Ok(Json(OverviewResponse {
        companies: tenants.len(),
        active_companies,
        suspended_companies,
        active_subscriptions: active,
        overdue_subscriptions: overdue,
        cancelled_subscriptions: cancelled,
        inactive_subscriptions: inactive,
        super_admins: admins.len(),
        new_companies_month,
        mrr: brl(mrr),
        annual_revenue: brl(annual),
        arpa: brl(arpa),
        top_plan,
        referrals: 0,
        revenue_months,
        recent_companies,
    }))
}
