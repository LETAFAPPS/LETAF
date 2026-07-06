use std::sync::Arc;
use rust_decimal::Decimal;

use chrono::Utc;
use uuid::Uuid;

use crate::error::CoreError;

use super::model::Plan;
use super::repository::PlanRepository;

/// Dados de entrada para criar/atualizar um plano (sem metadados internos).
pub struct PlanInput {
    pub name: String,
    pub amount: Decimal,
    pub period_months: i32,
    pub trial_days: i32,
    pub description: String,
    pub highlight_label: String,
    pub active: bool,
    pub sort_order: i32,
}

/// Regras do catálogo de planos (gestão pelo super admin). Valida entrada
/// (§11 — nunca confiar no frontend) e delega ao repository (§10).
pub struct PlanService {
    repo: Arc<dyn PlanRepository>,
}

impl PlanService {
    pub fn new(repo: Arc<dyn PlanRepository>) -> Self {
        Self { repo }
    }

    pub async fn find_all(&self) -> Result<Vec<Plan>, CoreError> {
        self.repo.find_all().await
    }

    pub async fn find_active(&self) -> Result<Vec<Plan>, CoreError> {
        self.repo.find_active().await
    }

    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<Plan>, CoreError> {
        self.repo.find_by_id(id).await
    }

    pub async fn create(&self, input: PlanInput) -> Result<Plan, CoreError> {
        let plan = build(Uuid::new_v4(), input)?;
        self.repo.create(&plan).await?;
        Ok(plan)
    }

    pub async fn update(&self, id: Uuid, input: PlanInput) -> Result<Plan, CoreError> {
        let existing = self
            .repo
            .find_by_id(id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Plano não encontrado".into()))?;
        let mut plan = build(id, input)?;
        plan.created_at = existing.created_at; // preserva criação
        self.repo.update(&plan).await?;
        Ok(plan)
    }

    pub async fn soft_delete(&self, id: Uuid) -> Result<(), CoreError> {
        self.repo.soft_delete(id).await
    }
}

/// Valida e monta um `Plan` a partir da entrada.
fn build(id: Uuid, input: PlanInput) -> Result<Plan, CoreError> {
    if input.name.trim().is_empty() {
        return Err(CoreError::Validation("Informe o nome do plano".into()));
    }
    if input.amount <= Decimal::ZERO {
        return Err(CoreError::Validation("O valor deve ser maior que zero".into()));
    }
    if input.period_months < 1 {
        return Err(CoreError::Validation("O período deve ser de ao menos 1 mês".into()));
    }
    if input.trial_days < 0 {
        return Err(CoreError::Validation("Período gratuito inválido".into()));
    }
    let now = Utc::now().naive_utc();
    Ok(Plan {
        id,
        name: input.name.trim().to_string(),
        amount: input.amount,
        period_months: input.period_months,
        trial_days: input.trial_days,
        description: input.description,
        highlight_label: input.highlight_label,
        active: input.active,
        sort_order: input.sort_order,
        created_at: now,
        updated_at: now,
        deleted_at: None,
    })
}
