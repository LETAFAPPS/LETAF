use async_trait::async_trait;
use chrono::{NaiveDate, NaiveDateTime};
use uuid::Uuid;

use super::model::{FinanceEntry, FinanceKind};
use crate::error::CoreError;

/// Acesso a dados de lançamentos financeiros.
///
/// Regras aplicadas (AI_RULES.md §10):
/// - Todas as queries filtram por `company_id` (isolamento multi-tenant).
/// - Métodos de sync incluídos (lançamentos sincronizam com o servidor).
/// - Operações em lote (`create_batch`) recebem `&[FinanceEntry]`
///   pra permitir transação atômica quando o service gerar parcelas
///   ou ocorrências de recorrência (AI_RULES.md §4.Transações).
#[async_trait]
pub trait FinanceRepository: Send + Sync {
    async fn find_by_id(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<Option<FinanceEntry>, CoreError>;

    /// Lista todos os lançamentos ativos (não-deletados) da empresa.
    /// Filtros por kind/status/intervalo de datas ficam no service
    /// ou na UI; aqui só `company_id`.
    async fn find_all(&self, company_id: Uuid) -> Result<Vec<FinanceEntry>, CoreError>;

    /// Filtra por tipo (Payable/Receivable). Atalho do listar para o
    /// caso comum (tab "A receber" / "A pagar").
    async fn find_by_kind(
        &self,
        company_id: Uuid,
        kind: FinanceKind,
    ) -> Result<Vec<FinanceEntry>, CoreError>;

    /// Lançamentos cujo `due_date` cai dentro da janela `[start, end]`.
    /// Usado pelo calendário mensal e pelo fluxo de caixa 30 dias.
    async fn find_in_range(
        &self,
        company_id: Uuid,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Vec<FinanceEntry>, CoreError>;

    /// Cria UM lançamento. Para parcelamento/recorrência o service
    /// usa [`create_batch`] em transação única.
    async fn create(&self, entry: &FinanceEntry) -> Result<(), CoreError>;

    /// Cria N lançamentos em uma única transação. Garante que todas
    /// as parcelas existam ou nenhuma (AI_RULES.md §4.Transações).
    async fn create_batch(&self, entries: &[FinanceEntry]) -> Result<(), CoreError>;

    async fn update(&self, entry: &FinanceEntry) -> Result<(), CoreError>;
    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError>;

    // ── Sync ──
    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<FinanceEntry>, CoreError>;
    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError>;
    async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
    ) -> Result<Vec<FinanceEntry>, CoreError>;
    /// Página do pull por keyset `(updated_at, id)` (default delega ao acima).
    async fn find_updated_since_paged(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
        _after_id: Uuid,
        _limit: i64,
    ) -> Result<Vec<FinanceEntry>, CoreError> {
        self.find_updated_since(company_id, since).await
    }
    async fn sync_upsert(&self, entry: &FinanceEntry) -> Result<(), CoreError>;
}
