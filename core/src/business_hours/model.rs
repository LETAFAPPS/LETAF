use serde::{Deserialize, Serialize};

use crate::entity::BaseFields;

/// Horário de funcionamento para um dia da semana.
///
/// Regras aplicadas (AI_RULES.md §6):
/// - Campos base obrigatórios (UUID, company_id, timestamps, synced)
/// - day_of_week: 0 = Domingo, 1 = Segunda … 6 = Sábado
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusinessHours {
    #[serde(flatten)]
    pub base: BaseFields,
    pub day_of_week: i32,
    pub open_time: String,
    pub close_time: String,
    pub is_open: bool,
}

impl BusinessHours {
    pub fn new(
        company_id: uuid::Uuid,
        day_of_week: i32,
        open_time: String,
        close_time: String,
        is_open: bool,
    ) -> Self {
        Self {
            base: BaseFields::new(company_id),
            day_of_week,
            open_time,
            close_time,
            is_open,
        }
    }
}
