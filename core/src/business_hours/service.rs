use std::sync::Arc;

use chrono::NaiveDateTime;
use uuid::Uuid;

use super::model::BusinessHours;
use super::repository::BusinessHoursRepository;
use crate::error::CoreError;

/// Service para o domínio BusinessHours.
///
/// Regras aplicadas (AI_RULES.md §1, §9, §11):
/// - Orquestração de regras de negócio
/// - Depende de repository via trait
/// - Validação de dados de entrada
pub struct BusinessHoursService {
    repo: Arc<dyn BusinessHoursRepository>,
}

impl BusinessHoursService {
    pub fn new(repo: Arc<dyn BusinessHoursRepository>) -> Self {
        Self { repo }
    }

    pub async fn find_all(&self, company_id: Uuid) -> Result<Vec<BusinessHours>, CoreError> {
        self.repo.find_all(company_id).await
    }

    /// Cria ou atualiza o horário de um dia da semana.
    pub async fn upsert(
        &self,
        company_id: Uuid,
        day_of_week: i32,
        open_time: String,
        close_time: String,
        is_open: bool,
    ) -> Result<BusinessHours, CoreError> {
        if !(0..=6).contains(&day_of_week) {
            return Err(CoreError::Validation("day_of_week deve ser entre 0 e 6".into()));
        }
        if !is_valid_time(&open_time) {
            return Err(CoreError::Validation(format!("open_time inválido: '{open_time}' (esperado HH:MM)")));
        }
        if !is_valid_time(&close_time) {
            return Err(CoreError::Validation(format!("close_time inválido: '{close_time}' (esperado HH:MM)")));
        }

        let hours = match self.repo.find_by_day(company_id, day_of_week).await? {
            Some(mut existing) => {
                existing.open_time = open_time;
                existing.close_time = close_time;
                existing.is_open = is_open;
                existing.base.updated_at = chrono::Utc::now().naive_utc();
                existing.base.synced = false;
                existing
            }
            None => BusinessHours::new(company_id, day_of_week, open_time, close_time, is_open),
        };

        self.repo.upsert(&hours).await?;
        Ok(hours)
    }

    /// Busca horários ainda não sincronizados (§7).
    pub async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<BusinessHours>, CoreError> {
        self.repo.find_unsynced(company_id).await
    }

    /// Marca horário como sincronizado (§7).
    pub async fn mark_synced(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        self.repo.mark_synced(company_id, id).await
    }

    /// Busca horários atualizados após o timestamp (§7 — sync pull).
    pub async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
    ) -> Result<Vec<BusinessHours>, CoreError> {
        self.repo.find_updated_since(company_id, since).await
    }

    /// Upsert de sincronização (§7.7 — last-write-wins).
    pub async fn sync_upsert(
        &self,
        company_id: Uuid,
        mut hours: BusinessHours,
    ) -> Result<(), CoreError> {
        if hours.base.company_id != company_id {
            return Err(CoreError::Validation("Company mismatch".into()));
        }
        hours.base.synced = true;
        self.repo.sync_upsert(&hours).await
    }
}

/// Valida que o horário segue o formato HH:MM (00:00–23:59).
fn is_valid_time(t: &str) -> bool {
    let bytes = t.as_bytes();
    if bytes.len() != 5 || bytes[2] != b':' {
        return false;
    }
    let hh: u8 = t[..2].parse().unwrap_or(99);
    let mm: u8 = t[3..].parse().unwrap_or(99);
    hh <= 23 && mm <= 59
}
