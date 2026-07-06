use chrono::NaiveDateTime;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Plano do catálogo gerido pelo super admin (nível PLATAFORMA — global,
/// sem `company_id`; exceção documentada ao multi-tenant, como o super
/// admin). As lojas leem os planos ativos para exibir/assinar.
///
/// Regras (AI_RULES.md §6/§10): id UUID, soft delete (`deleted_at`),
/// acesso a dados só via repository. `amount` é o valor por ciclo (R$),
/// `period_months` o intervalo da cobrança e `trial_days` o período grátis
/// antes da 1ª cobrança (Fase 2 — billing).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: Uuid,
    pub name: String,
    pub amount: Decimal,
    pub period_months: i32,
    pub trial_days: i32,
    pub description: String,
    pub highlight_label: String,
    pub active: bool,
    pub sort_order: i32,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    #[serde(default)]
    pub deleted_at: Option<NaiveDateTime>,
}

impl Plan {
    /// Mensalidade efetiva (R$/mês) = valor por ciclo ÷ meses.
    pub fn monthly_price(&self) -> Decimal {
        if self.period_months > 0 {
            self.amount / Decimal::from(self.period_months)
        } else {
            self.amount
        }
    }
}
