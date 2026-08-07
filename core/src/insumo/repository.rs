//! Acesso a dados de Insumo (AI_RULES §10 — só via repository, via trait).
//! Espelha o essencial de `product::repository`: CRUD, ledger de movimentos e
//! os métodos de sincronização (push/pull keyset + upsert LWW + apply delta).

use async_trait::async_trait;
use chrono::NaiveDateTime;
use uuid::Uuid;

use super::model::{Insumo, InsumoMovement};
use crate::error::CoreError;
use crate::product::repository::StockAdjustResult;

#[async_trait]
pub trait InsumoRepository: Send + Sync {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Insumo>, CoreError>;
    async fn find_all(&self, company_id: Uuid) -> Result<Vec<Insumo>, CoreError>;
    async fn create(&self, insumo: &Insumo) -> Result<(), CoreError>;

    /// Edição atômica: metadados + delta de estoque (com movimento no ledger)
    /// numa única transação. `stock_delta = alvo - atual` (0 = sem mudança).
    async fn update_atomic(&self, insumo: &Insumo, stock_delta: f64) -> Result<(), CoreError>;

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError>;

    /// Aplica `delta` ao estoque numa única `UPDATE` atômica, gravando o
    /// movimento com `reason` no ledger. `Insufficient` se levaria a < 0.
    async fn try_adjust_stock(
        &self,
        company_id: Uuid,
        insumo_id: Uuid,
        delta: f64,
        reason: &str,
    ) -> Result<StockAdjustResult, CoreError>;

    // ── Sincronização (§7) ──────────────────────────────────────────
    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Insumo>, CoreError>;
    async fn mark_synced(
        &self,
        company_id: Uuid,
        id: Uuid,
        updated_at: NaiveDateTime,
    ) -> Result<(), CoreError>;
    async fn sync_upsert(&self, insumo: &Insumo) -> Result<(), CoreError>;
    async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
    ) -> Result<Vec<Insumo>, CoreError>;
    async fn find_updated_since_paged(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
        _after_id: Uuid,
        _limit: i64,
    ) -> Result<Vec<Insumo>, CoreError> {
        self.find_updated_since(company_id, since).await
    }

    // ── Ledger de movimentos (append-only, §6/§7) ───────────────────
    async fn find_unsynced_movements(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<InsumoMovement>, CoreError>;
    async fn mark_movement_synced(
        &self,
        company_id: Uuid,
        id: Uuid,
        updated_at: NaiveDateTime,
    ) -> Result<(), CoreError>;
    /// Aplica um movimento IDEMPOTENTE: insere (no-op se id já existe) e, só na
    /// 1ª vez, aplica `stock += delta` ao insumo na mesma transação.
    async fn apply_movement(&self, movement: &InsumoMovement) -> Result<(), CoreError>;
    /// Grava um movimento do PULL sem tocar em `stock_quantity` (histórico).
    async fn insert_synced_movement(
        &self,
        movement: &InsumoMovement,
    ) -> Result<(), CoreError>;
    async fn find_movements_updated_since(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
    ) -> Result<Vec<InsumoMovement>, CoreError>;
    async fn find_movements_updated_since_paged(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
        _after_id: Uuid,
        _limit: i64,
    ) -> Result<Vec<InsumoMovement>, CoreError> {
        self.find_movements_updated_since(company_id, since).await
    }
}
