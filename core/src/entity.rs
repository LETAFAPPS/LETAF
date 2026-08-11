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

/// Skew máximo tolerado para um `updated_at` vindo de cliente antes de
/// considerá-lo abusivo. 1 dia cobre drift de relógio real (minutos/horas)
/// com folga — nenhum terminal legítimo carimba um dia à frente.
const MAX_FUTURE_SKEW_DAYS: i64 = 1;

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

    /// Limita `updated_at` a `agora + skew` (§11 — anti-freeze do LWW). Um
    /// cliente/desktop adulterado poderia enviar `updated_at` no ano 9999 para
    /// VENCER todo conflito futuro (§7.7) e tornar a linha imutável, perdendo
    /// silenciosamente as edições legítimas de outros terminais. Só altera
    /// valores absurdamente no futuro — um timestamp legítimo (servidor ou
    /// desktop, ambos com relógio ~atual) nunca é tocado. Aplicado no
    /// `sync_upsert` (caminho não-confiável); no-op no create local.
    pub fn clamp_future_updated_at(&mut self) {
        let cap = chrono::Utc::now().naive_utc() + chrono::Duration::days(MAX_FUTURE_SKEW_DAYS);
        if self.updated_at > cap {
            self.updated_at = cap;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    #[test]
    fn clamp_corta_futuro_absurdo_e_preserva_legitimo() {
        // updated_at no ano 9999 → clampado para <= agora + skew.
        let mut b = BaseFields::new(Uuid::nil());
        b.updated_at = chrono::NaiveDate::from_ymd_opt(9999, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();
        b.clamp_future_updated_at();
        assert!(b.updated_at <= Utc::now().naive_utc() + Duration::days(MAX_FUTURE_SKEW_DAYS + 1));
        assert!(b.updated_at.and_utc().timestamp() < 253_402_300_800); // < ano 9999

        // Timestamp legítimo (agora) é preservado intacto.
        let mut b2 = BaseFields::new(Uuid::nil());
        let legit = Utc::now().naive_utc();
        b2.updated_at = legit;
        b2.clamp_future_updated_at();
        assert_eq!(b2.updated_at, legit);
    }
}
