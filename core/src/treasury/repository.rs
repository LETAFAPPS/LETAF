use async_trait::async_trait;
use chrono::NaiveDateTime;
use uuid::Uuid;

use super::model::{Treasury, TreasuryMovement};
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
///   (§7.7) — vale para a carteira e para os movimentos manuais.
/// - Movimentos manuais são append-only: só `create_movement` insere;
///   UPDATE apenas para marcar `synced`.
#[async_trait]
pub trait TreasuryRepository: Send + Sync {
    /// Carteira da empresa (singleton) — `None` se ainda não criada.
    async fn find_by_company(&self, company_id: Uuid) -> Result<Option<Treasury>, CoreError>;

    async fn create(&self, treasury: &Treasury) -> Result<(), CoreError>;

    /// Atualiza a carteira (hoje: meta de reserva e observação). O
    /// service já preparou `updated_at`/`synced` no estado recebido.
    async fn update(&self, treasury: &Treasury) -> Result<(), CoreError>;

    // ── Movimentos manuais ──

    /// Insere um movimento manual (append-only).
    async fn create_movement(&self, movement: &TreasuryMovement) -> Result<(), CoreError>;

    /// Movimentos manuais da empresa, mais recentes primeiro.
    async fn find_movements(
        &self,
        company_id: Uuid,
        limit: i64,
    ) -> Result<Vec<TreasuryMovement>, CoreError>;

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

    // ── Sync — movimentos ──

    async fn find_unsynced_movements(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<TreasuryMovement>, CoreError>;
    async fn mark_movement_synced(
        &self,
        company_id: Uuid,
        id: Uuid,
        updated_at: NaiveDateTime,
    ) -> Result<(), CoreError>;
    async fn find_movements_updated_since(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
    ) -> Result<Vec<TreasuryMovement>, CoreError>;
    /// Página do pull de movimentos por keyset `(updated_at, id)`.
    ///
    /// O ledger da tesouraria é append-only e CRESCE: puxá-lo inteiro numa
    /// requisição só (era o que acontecia) acabaria estourando o timeout
    /// de 10 s do cliente, e aí a entidade congelaria de vez. Default
    /// delega ao não paginado; só o Postgres sobrescreve.
    async fn find_movements_updated_since_paged(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
        _after_id: Uuid,
        _limit: i64,
    ) -> Result<Vec<TreasuryMovement>, CoreError> {
        self.find_movements_updated_since(company_id, since).await
    }

    async fn sync_upsert_movement(&self, movement: &TreasuryMovement) -> Result<(), CoreError>;
}
