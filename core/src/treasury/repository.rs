use async_trait::async_trait;
use chrono::NaiveDateTime;
use uuid::Uuid;

use super::model::Treasury;
use crate::error::CoreError;

/// Acesso a dados da carteira do estabelecimento.
///
/// Regras aplicadas (AI_RULES.md §4, §10):
/// - Todas as queries filtram por `company_id` (multi-tenant).
/// - Singleton por empresa: a busca canônica é `find_by_company`
///   (unique em `company_id` na migration).
/// - `mark_synced` é CONDICIONAL ao `updated_at` enviado
///   (`WHERE ... AND updated_at = ?`) — contrato do sync worker (§7.6):
///   se o registro mudou enquanto o push estava em voo, 0 linhas são
///   afetadas e ele é reenviado no próximo ciclo.
/// - `sync_upsert` resolve conflito por last-write-wins via `updated_at`
///   (§7.7).
#[async_trait]
pub trait TreasuryRepository: Send + Sync {
    /// Carteira da empresa (singleton) — `None` se ainda não criada.
    async fn find_by_company(&self, company_id: Uuid) -> Result<Option<Treasury>, CoreError>;

    async fn create(&self, treasury: &Treasury) -> Result<(), CoreError>;

    // ── Sync ──
    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Treasury>, CoreError>;
    async fn mark_synced(
        &self,
        company_id: Uuid,
        id: Uuid,
        updated_at: NaiveDateTime,
    ) -> Result<(), CoreError>;
    async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
    ) -> Result<Vec<Treasury>, CoreError>;
    async fn sync_upsert(&self, treasury: &Treasury) -> Result<(), CoreError>;
}
