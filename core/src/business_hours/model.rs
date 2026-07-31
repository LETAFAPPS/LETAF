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
        // Id derivado de `(company_id, day_of_week)`: instalação nova
        // configurando horários offline não colide com o que já existe
        // no servidor (§7).
        let mut base = BaseFields::new(company_id);
        base.id = crate::deterministic_id::business_hours(company_id, day_of_week);
        Self {
            base,
            day_of_week,
            open_time,
            close_time,
            is_open,
        }
    }
}
