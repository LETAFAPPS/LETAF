use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Uma entrada da trilha de auditoria.
///
/// Regras aplicadas (AI_RULES.md §6, §11):
/// - `id` UUID (sem auto-incremento).
/// - Registro IMUTÁVEL: nunca é editado nem removido (não há soft delete);
///   uma trilha que pode ser alterada não serve como trilha.
/// - `actor_name` / `target_label` são desnormalizados de propósito: o log
///   precisa continuar legível mesmo se a empresa/usuário for excluído.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: Uuid,
    /// Super admin que executou a ação (`sub` do JWT).
    pub actor_id: Uuid,
    pub actor_name: String,
    /// Ação no formato `<recurso>.<verbo>` (ex.: "company.suspend").
    pub action: String,
    /// Tipo do alvo ("company", "subscription", "invoice", "plan", "admin").
    pub target_type: String,
    pub target_id: Option<Uuid>,
    /// Rótulo legível do alvo (nome da empresa, do plano...).
    pub target_label: String,
    /// Detalhe livre em texto (ex.: "status: active → cancelled").
    pub details: String,
    pub created_at: NaiveDateTime,
}

impl AuditEntry {
    pub fn new(
        actor_id: Uuid,
        actor_name: String,
        action: String,
        target_type: String,
        target_id: Option<Uuid>,
        target_label: String,
        details: String,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            actor_id,
            actor_name,
            action,
            target_type,
            target_id,
            target_label,
            details,
            created_at: chrono::Utc::now().naive_utc(),
        }
    }
}
