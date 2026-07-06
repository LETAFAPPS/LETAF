use async_trait::async_trait;
use uuid::Uuid;

use crate::error::CoreError;

use super::model::Plan;

/// Acesso a dados do catálogo de planos (§10 — só via repository).
/// Implementado no servidor (PostgreSQL). Global (sem company_id).
#[async_trait]
pub trait PlanRepository: Send + Sync {
    /// Todos os planos não removidos, ordenados por `sort_order` (gestão).
    async fn find_all(&self) -> Result<Vec<Plan>, CoreError>;

    /// Apenas os planos ATIVOS (vitrine das lojas).
    async fn find_active(&self) -> Result<Vec<Plan>, CoreError>;

    async fn find_by_id(&self, id: Uuid) -> Result<Option<Plan>, CoreError>;

    async fn create(&self, plan: &Plan) -> Result<(), CoreError>;

    async fn update(&self, plan: &Plan) -> Result<(), CoreError>;

    /// Remoção lógica (soft delete).
    async fn soft_delete(&self, id: Uuid) -> Result<(), CoreError>;
}
