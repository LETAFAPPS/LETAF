use async_trait::async_trait;
use uuid::Uuid;

use super::model::Printer;
use crate::error::CoreError;

/// Trait de acesso a dados para Printer.
///
/// Regras aplicadas (AI_RULES.md §10):
/// - Acesso ao banco somente via repository.
/// - Todas as queries filtram por `company_id` para isolamento
///   multi-tenant, mesmo sabendo que impressora é per-device — assim
///   se um dia o desktop suportar trocar de empresa no mesmo binário
///   (multi-loja), a separação já existe.
///
/// **Não inclui métodos de sync** (`find_unsynced`, `sync_upsert`, etc.)
/// porque impressora não sincroniza com servidor. O `synced` da
/// BaseFields fica sempre `true` para o SyncWorker pular o registro.
#[async_trait]
pub trait PrinterRepository: Send + Sync {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Printer>, CoreError>;
    async fn find_all(&self, company_id: Uuid) -> Result<Vec<Printer>, CoreError>;
    /// Devolve a impressora marcada como padrão para o `kind`
    /// solicitado (ou `None` se nenhuma cadastrada). Usado pela
    /// camada de impressão para resolver `system_name` em runtime.
    async fn find_default(&self, company_id: Uuid, kind: &str) -> Result<Option<Printer>, CoreError>;
    /// Lista TODAS as impressoras de um determinado `kind` (não só a
    /// padrão). Usado pelo roteamento por categoria: precisamos
    /// percorrer todas as impressoras `kind=kitchen` para decidir
    /// quem imprime cada item.
    async fn find_by_kind(&self, company_id: Uuid, kind: &str) -> Result<Vec<Printer>, CoreError>;
    async fn create(&self, printer: &Printer) -> Result<(), CoreError>;
    async fn update(&self, printer: &Printer) -> Result<(), CoreError>;
    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError>;
    /// Marca `id` como padrão para o `kind` e desmarca **todas** as
    /// outras impressoras do mesmo kind na mesma empresa. Deve ser
    /// executado em transação para evitar janela com 0 ou 2 padrões.
    async fn set_default(&self, company_id: Uuid, id: Uuid, kind: &str) -> Result<(), CoreError>;
}
