use async_trait::async_trait;
use chrono::NaiveDateTime;
use uuid::Uuid;

use super::model::{CashMovement, CashSession};
use crate::error::CoreError;

/// Acesso a dados de sessões de caixa.
///
/// Regras aplicadas (AI_RULES.md §10):
/// - Todas as queries filtram por `company_id` (isolamento multi-tenant).
/// - Inclui métodos de sync (`find_unsynced`, `sync_upsert`,
///   `find_updated_since`) porque sessões sincronizam com o servidor.
#[async_trait]
pub trait CashSessionRepository: Send + Sync {
    async fn find_by_id(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<Option<CashSession>, CoreError>;

    /// Devolve a sessão Open atual (no máximo uma por company), ou
    /// `None` se o caixa estiver fechado. Service usa pra rejeitar
    /// `open_session` duplicado.
    async fn find_active(&self, company_id: Uuid) -> Result<Option<CashSession>, CoreError>;

    /// Histórico das últimas `limit` sessões (Closed + Open), ordenadas
    /// por `opened_at` DESC. Usado pela aba Caixa quando fechado.
    async fn find_recent(
        &self,
        company_id: Uuid,
        limit: i64,
    ) -> Result<Vec<CashSession>, CoreError>;

    async fn create(&self, session: &CashSession) -> Result<(), CoreError>;
    async fn update(&self, session: &CashSession) -> Result<(), CoreError>;

    // ── Sync ──
    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<CashSession>, CoreError>;
    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError>;
    async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
    ) -> Result<Vec<CashSession>, CoreError>;
    /// Página do pull por keyset `(updated_at, id)` (default delega ao acima).
    async fn find_updated_since_paged(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
        _after_id: Uuid,
        _limit: i64,
    ) -> Result<Vec<CashSession>, CoreError> {
        self.find_updated_since(company_id, since).await
    }
    async fn sync_upsert(&self, session: &CashSession) -> Result<(), CoreError>;
}

/// Acesso a dados de movimentos de caixa (livro-razão).
///
/// Regras aplicadas (AI_RULES.md §10):
/// - Movimentos são *append-only* na lógica do service (UPDATE só pra
///   marcar `synced`). O repository permite update por completude do
///   contrato.
#[async_trait]
pub trait CashMovementRepository: Send + Sync {
    async fn find_by_id(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<Option<CashMovement>, CoreError>;

    /// Movimentos de uma sessão em ordem cronológica (ASC) — base do
    /// `SessionSummary` e da timeline da UI.
    async fn find_by_session(
        &self,
        company_id: Uuid,
        session_id: Uuid,
    ) -> Result<Vec<CashMovement>, CoreError>;

    async fn create(&self, movement: &CashMovement) -> Result<(), CoreError>;

    // ── Sync ──
    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<CashMovement>, CoreError>;
    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError>;
    async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
    ) -> Result<Vec<CashMovement>, CoreError>;
    /// Página do pull por keyset `(updated_at, id)` (default delega ao acima).
    async fn find_updated_since_paged(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
        _after_id: Uuid,
        _limit: i64,
    ) -> Result<Vec<CashMovement>, CoreError> {
        self.find_updated_since(company_id, since).await
    }
    async fn sync_upsert(&self, movement: &CashMovement) -> Result<(), CoreError>;
}
