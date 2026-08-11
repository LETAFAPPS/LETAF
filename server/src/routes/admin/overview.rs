//! Painel do super admin — visão geral (KPIs cross-tenant).

use axum::extract::State;
use axum::Json;
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use std::collections::HashMap;

use chrono::Datelike;

use crate::context::AppState;
use crate::error::ServerError;
use crate::middleware::auth::AuthClaims;

use super::{brl, tenants};
use serde::Serialize;
use uuid::Uuid;

/// Abreviações de mês em pt-BR (índice 0 = janeiro).
const MESES_ABBR: [&str; 12] = [
    "jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez",
];

// ── Painel (visão geral) ─────────────────────────────────────────────────
/// Um ponto do gráfico de receita anual (um mês do ano-calendário), com o
/// valor do ano atual e do ano anterior lado a lado (comparativo).
#[derive(Serialize)]
pub(super) struct RevenuePoint {
    /// Rótulo do mês em pt-BR ("jan", "fev", ...).
    label: String,
    /// Valor do mês no ANO ATUAL (numérico, para a altura da barra).
    current: f64,
    /// Valor do mesmo mês no ANO ANTERIOR (numérico).
    previous: f64,
    /// Valores já em pt-BR ("R$ 1.234,56") para tooltip.
    current_brl: String,
    previous_brl: String,
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
    /// Receita realizada (faturas pagas) no ANO ATUAL (jan–dez), em pt-BR.
    annual_revenue: String,
    /// Ticket médio por assinatura ativa (ARPA), em pt-BR.
    arpa: String,
    /// Nome do plano mais assinado entre as assinaturas ativas ("—" se nenhum).
    top_plan: String,
    /// Indicações de novos estabelecimentos — funcionalidade FUTURA
    /// (placeholder; hoje sempre 0). Mantido no contrato para evolução.
    referrals: usize,
    /// Ano atual e anterior (rótulos da legenda do gráfico).
    year: i32,
    prev_year: i32,
    /// Série do gráfico: 12 meses (jan–dez), ano atual × ano anterior.
    revenue_months: Vec<RevenuePoint>,
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

    // Receita por MÊS-CALENDÁRIO (jan–dez), comparando o ANO ATUAL com o
    // ANTERIOR. Faturas PAGAS somadas por mês em UMA agregação SQL cross-tenant
    // (§13) — substitui o antigo laço O(tenants) que materializava TODO o
    // histórico de faturas de cada empresa.
    let today = chrono::Utc::now().naive_utc().date();
    let year = today.year();
    let prev_year = year - 1;
    let mut current = [Decimal::ZERO; 12];
    let mut previous = [Decimal::ZERO; 12];
    let since = chrono::NaiveDate::from_ymd_opt(prev_year, 1, 1).unwrap_or(today);
    for (y, m, sum) in state
        .subscription_service
        .paid_revenue_by_month_since(since)
        .await
        .unwrap_or_default()
    {
        let idx = ((m.clamp(1, 12)) - 1) as usize;
        let val = Decimal::from_f64(sum).unwrap_or(Decimal::ZERO);
        if y == year {
            current[idx] = val;
        } else if y == prev_year {
            previous[idx] = val;
        }
    }
    // "Receita no ano" = total do ano atual (jan–dez).
    let annual: Decimal = current.iter().copied().sum();
    let revenue_months: Vec<RevenuePoint> = (0..12)
        .map(|i| RevenuePoint {
            label: MESES_ABBR[i].to_string(),
            current: current[i].to_f64().unwrap_or(0.0),
            previous: previous[i].to_f64().unwrap_or(0.0),
            current_brl: brl(current[i]),
            previous_brl: brl(previous[i]),
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
        year,
        prev_year,
        revenue_months,
    }))
}
