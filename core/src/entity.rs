use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Campos obrigatórios presentes em toda entidade do sistema.
///
/// Regras aplicadas (AI_RULES.md §6):
/// - id: UUID (sem auto-incremento)
/// - company_id: isolamento multi-tenant
/// - created_at / updated_at: timestamps obrigatórios
/// - deleted_at: soft delete
/// - synced: controle de sincronização offline-first
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseFields {
    pub id: Uuid,
    pub company_id: Uuid,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub deleted_at: Option<NaiveDateTime>,
    pub synced: bool,
}

impl BaseFields {
    pub fn new(company_id: Uuid) -> Self {
        let now = chrono::Utc::now().naive_utc();
        Self {
            id: Uuid::new_v4(),
            company_id,
            created_at: now,
            updated_at: now,
            deleted_at: None,
            synced: false,
        }
    }
}
