use async_trait::async_trait;

use crate::error::CoreError;

use super::model::AuditEntry;

/// Persistência da trilha de auditoria (§10 — acesso a dados só via
/// repository, abstraído por trait).
///
/// Só há inserção e leitura: a trilha é imutável (§11).
#[async_trait]
pub trait AuditRepository: Send + Sync {
    async fn create(&self, entry: &AuditEntry) -> Result<(), CoreError>;
    /// Entradas mais recentes primeiro, no máximo `limit`.
    async fn find_recent(&self, limit: i64) -> Result<Vec<AuditEntry>, CoreError>;
}
