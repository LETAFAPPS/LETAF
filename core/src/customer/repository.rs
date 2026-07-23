use async_trait::async_trait;
use chrono::NaiveDateTime;
use uuid::Uuid;

use super::model::Customer;
use crate::error::CoreError;

/// Trait de acesso a dados para Customer.
///
/// Regras aplicadas (AI_RULES.md §10):
/// - Acesso ao banco somente via repository
/// - Usar traits para abstração
///
/// Cada implementação concreta (PostgreSQL, SQLite) ficará
/// na camada correspondente (server/repository, desktop/repository).
#[async_trait]
pub trait CustomerRepository: Send + Sync {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Customer>, CoreError>;
    async fn find_all(&self, company_id: Uuid) -> Result<Vec<Customer>, CoreError>;

    /// Conta os registros ATIVOS da empresa (para o painel do super admin).
    ///
    /// Implementação padrão carrega a lista — suficiente para o SQLite
    /// local, que é pequeno. O PostgreSQL sobrescreve com `COUNT(*)` para
    /// não trazer blobs/linhas inteiras só para contar (§13).
    async fn count_all(&self, company_id: Uuid) -> Result<i64, CoreError> {
        Ok(self.find_all(company_id).await?.len() as i64)
    }

    async fn find_by_email(&self, company_id: Uuid, email: &str) -> Result<Option<Customer>, CoreError>;
    async fn create(&self, customer: &Customer) -> Result<(), CoreError>;
    async fn update(&self, customer: &Customer) -> Result<(), CoreError>;
    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError>;
    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Customer>, CoreError>;
    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError>;

    /// Upsert de sincronização (§7.7 — last-write-wins via updated_at).
    async fn sync_upsert(&self, customer: &Customer) -> Result<(), CoreError>;

    /// Busca entidades atualizadas após o timestamp (§7 — sync pull).
    async fn find_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<Customer>, CoreError>;

    /// Página do pull por keyset `(updated_at, id)` — ver
    /// `ProductRepository::find_updated_since_paged`. Default delega ao
    /// não-paginado; só o Postgres sobrescreve.
    async fn find_updated_since_paged(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
        _after_id: Uuid,
        _limit: i64,
    ) -> Result<Vec<Customer>, CoreError> {
        self.find_updated_since(company_id, since).await
    }
}
