use async_trait::async_trait;
use chrono::NaiveDateTime;
use uuid::Uuid;

use super::model::Coupon;
use crate::error::CoreError;

/// Trait de acesso a dados para Coupon.
///
/// Regras aplicadas (AI_RULES.md §10):
/// - Acesso ao banco somente via repository.
/// - Todas as operações filtradas por `company_id` (isolamento
///   multi-tenant — §11).
#[async_trait]
pub trait CouponRepository: Send + Sync {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Coupon>, CoreError>;
    /// Busca por código (case-insensitive já normalizado em MAIÚSCULAS)
    /// — usado para garantir unicidade do código por empresa.
    async fn find_by_code(&self, company_id: Uuid, code: &str) -> Result<Option<Coupon>, CoreError>;
    async fn find_all(&self, company_id: Uuid) -> Result<Vec<Coupon>, CoreError>;
    /// Apenas cupons ativos (uso público / aplicação no checkout).
    async fn find_active(&self, company_id: Uuid) -> Result<Vec<Coupon>, CoreError>;
    async fn create(&self, coupon: &Coupon) -> Result<(), CoreError>;
    async fn update(&self, coupon: &Coupon) -> Result<(), CoreError>;
    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError>;
    async fn set_active(&self, company_id: Uuid, id: Uuid, active: bool) -> Result<(), CoreError>;

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Coupon>, CoreError>;
    async fn mark_synced(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError>;
    async fn sync_upsert(&self, coupon: &Coupon) -> Result<(), CoreError>;
    async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
    ) -> Result<Vec<Coupon>, CoreError>;
}
