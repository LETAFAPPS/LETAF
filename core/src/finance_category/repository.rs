use async_trait::async_trait;
use chrono::NaiveDateTime;
use uuid::Uuid;

use super::model::FinanceCategory;
use crate::error::CoreError;

/// Acesso a dados de categorias financeiras.
///
/// Regras aplicadas (AI_RULES.md §10):
/// - Todas as queries filtram por `company_id` (isolamento multi-tenant).
/// - Inclui métodos de sync porque categorias sincronizam com o
///   servidor (modelo idêntico ao de `category` / `subcategory`).
#[async_trait]
pub trait FinanceCategoryRepository: Send + Sync {
    async fn find_by_id(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<Option<FinanceCategory>, CoreError>;

    /// Devolve todas as categorias ativas (não-deletadas) da empresa.
    /// Usado pelo combobox de categorias no formulário de lançamento;
    /// o filtro por escopo (Payable/Receivable) acontece no service ou
    /// na UI (decisão de produto, não de banco).
    async fn find_all(&self, company_id: Uuid) -> Result<Vec<FinanceCategory>, CoreError>;

    async fn create(&self, category: &FinanceCategory) -> Result<(), CoreError>;
    async fn update(&self, category: &FinanceCategory) -> Result<(), CoreError>;

    /// Remoção lógica (soft delete) — marca `deleted_at = now` e
    /// `synced = false` (vide AI_RULES.md §6, §7.3).
    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError>;

    // ── Sync ──
    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<FinanceCategory>, CoreError>;
    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError>;
    async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
    ) -> Result<Vec<FinanceCategory>, CoreError>;
    async fn sync_upsert(&self, category: &FinanceCategory) -> Result<(), CoreError>;
}
