use async_trait::async_trait;
use chrono::NaiveDateTime;
use uuid::Uuid;

use super::model::BusinessHours;
use crate::error::CoreError;

/// Trait de acesso a dados para BusinessHours.
///
/// Regras aplicadas (AI_RULES.md §10):
/// - Acesso ao banco somente via repository
/// - Usar traits para abstração
#[async_trait]
pub trait BusinessHoursRepository: Send + Sync {
    async fn find_all(&self, company_id: Uuid) -> Result<Vec<BusinessHours>, CoreError>;
    async fn find_by_day(&self, company_id: Uuid, day_of_week: i32) -> Result<Option<BusinessHours>, CoreError>;
    async fn upsert(&self, hours: &BusinessHours) -> Result<(), CoreError>;
    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<BusinessHours>, CoreError>;
    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError>;

    /// Upsert de sincronização (§7.7 — last-write-wins via updated_at).
    async fn sync_upsert(&self, hours: &BusinessHours) -> Result<(), CoreError>;

    /// Busca entidades atualizadas após o timestamp (§7 — sync pull).
    async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
    ) -> Result<Vec<BusinessHours>, CoreError>;
}
