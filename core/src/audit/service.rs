use std::sync::Arc;

use uuid::Uuid;

use crate::error::CoreError;

use super::model::AuditEntry;
use super::repository::AuditRepository;

/// Serviço da trilha de auditoria.
///
/// Regras aplicadas (AI_RULES.md §8, §11): responsabilidade única — só
/// registra e lista. O `record` NUNCA deve derrubar a operação auditada:
/// quem chama trata o erro como best-effort (loga e segue).
pub struct AuditService {
    repo: Arc<dyn AuditRepository>,
}

impl AuditService {
    pub fn new(repo: Arc<dyn AuditRepository>) -> Self {
        Self { repo }
    }

    /// Registra uma ação. `details` pode ser vazio.
    #[allow(clippy::too_many_arguments)]
    pub async fn record(
        &self,
        actor_id: Uuid,
        actor_name: String,
        action: &str,
        target_type: &str,
        target_id: Option<Uuid>,
        target_label: String,
        details: String,
    ) -> Result<(), CoreError> {
        let entry = AuditEntry::new(
            actor_id,
            actor_name,
            action.to_string(),
            target_type.to_string(),
            target_id,
            target_label,
            details,
        );
        self.repo.create(&entry).await
    }

    /// Lista as entradas mais recentes (limite defensivo em 500).
    pub async fn find_recent(&self, limit: i64) -> Result<Vec<AuditEntry>, CoreError> {
        self.repo.find_recent(limit.clamp(1, 500)).await
    }
}
