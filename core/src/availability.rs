//! Disponibilidade por horário — funções PURAS reutilizáveis (§8), fonte da
//! verdade do backend (checkout). A janela é `[open, close)` e trata a virada
//! de meia-noite (ex.: 18:00→02:00). O web repete a mesma lógica só como dica
//! de UI (`web::availability`), coberta por teste de paridade.
//!
//! O "agora" da loja é derivado do agora em UTC + o offset fixo da empresa
//! (`Company::utc_offset_minutes`) — sem depender do relógio do cliente, que é
//! não-confiável (§11).

use chrono::{Datelike, Duration, NaiveDateTime, Timelike};
use serde::Deserialize;

use crate::business_hours::model::BusinessHours;

/// Um dia do `availability_schedule` JSON do produto.
#[derive(Deserialize)]
struct AvailabilityDay {
    day: i32,
    open: String,
    close: String,
    active: bool,
}

/// `(dia_da_semana 0=Dom..6=Sáb, minutos desde a meia-noite)` no fuso da loja,
/// a partir do agora em UTC e do offset fixo (minutos, ex.: -180 = BRT).
pub fn local_now(now_utc: NaiveDateTime, utc_offset_minutes: i32) -> (i32, i32) {
    let local = now_utc + Duration::minutes(utc_offset_minutes as i64);
    let day = local.weekday().num_days_from_sunday() as i32;
    let mins = (local.hour() * 60 + local.minute()) as i32;
    (day, mins)
}

/// Converte `"HH:MM"` em minutos desde a meia-noite.
fn hhmm_to_minutes(s: &str) -> Option<i32> {
    let (h, m) = s.split_once(':')?;
    let (h, m): (i32, i32) = (h.parse().ok()?, m.parse().ok()?);
    if !(0..24).contains(&h) || !(0..60).contains(&m) {
        return None;
    }
    Some(h * 60 + m)
}

/// `true` se `mins` cai na janela `[open, close)`, tratando a virada de
/// meia-noite (`close <= open` cruza 00:00; `close == open` = 24h).
fn in_window(mins: i32, open: i32, close: i32) -> bool {
    if close > open {
        mins >= open && mins < close
    } else {
        mins >= open || mins < close
    }
}

/// `true` se o produto está disponível no instante `(day, mins)`. Schedule
/// vazio/ausente/inválido → disponível (degradação graciosa, como no web).
pub fn is_product_available(schedule: Option<&str>, day: i32, mins: i32) -> bool {
    let Some(s) = schedule.filter(|s| !s.is_empty()) else {
        return true;
    };
    let Ok(entries) = serde_json::from_str::<Vec<AvailabilityDay>>(s) else {
        return true;
    };
    // Dia inativo OU sem entrada no schedule → indisponível (paridade exata
    // com `web::availability::is_available_now`, travada por teste).
    match entries.iter().find(|e| e.day == day) {
        Some(e) if e.active => {
            let open = hhmm_to_minutes(&e.open).unwrap_or(0);
            let close = hhmm_to_minutes(&e.close).unwrap_or(24 * 60);
            in_window(mins, open, close)
        }
        _ => false,
    }
}

/// `true` se a loja está aberta no instante `(day, mins)`. Respeita o override
/// manual ("open"/"closed"). Sem horários cadastrados → aberta (como no web).
pub fn is_store_open(
    hours: &[BusinessHours],
    store_override: &str,
    day: i32,
    mins: i32,
) -> bool {
    match store_override {
        "open" => return true,
        "closed" => return false,
        _ => {}
    }
    if hours.is_empty() {
        return true;
    }
    match hours.iter().find(|h| h.day_of_week == day) {
        Some(h) if h.is_open => {
            let open = hhmm_to_minutes(&h.open_time).unwrap_or(0);
            let close = hhmm_to_minutes(&h.close_time).unwrap_or(24 * 60);
            in_window(mins, open, close)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_now_aplica_offset() {
        // 2026-01-05 (segunda) 01:00 UTC, offset -180 → domingo 22:00 local.
        let utc = NaiveDateTime::parse_from_str("2026-01-05 01:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        let (day, mins) = local_now(utc, -180);
        assert_eq!(day, 0); // domingo
        assert_eq!(mins, 22 * 60);
    }

    #[test]
    fn produto_sem_schedule_disponivel() {
        assert!(is_product_available(None, 1, 600));
        assert!(is_product_available(Some(""), 1, 600));
        assert!(is_product_available(Some("lixo"), 1, 600));
    }

    #[test]
    fn produto_janela_normal_e_meia_noite() {
        let s = r#"[{"day":1,"open":"08:00","close":"12:00","active":true}]"#;
        assert!(is_product_available(Some(s), 1, 600)); // 10:00 dentro
        assert!(!is_product_available(Some(s), 1, 60)); // 01:00 fora
        assert!(!is_product_available(Some(s), 2, 600)); // dia sem entrada → indisponível (paridade web)
        let night = r#"[{"day":5,"open":"18:00","close":"02:00","active":true}]"#;
        assert!(is_product_available(Some(night), 5, 60)); // 01:00 dentro (após meia-noite)
        assert!(!is_product_available(Some(night), 5, 720)); // 12:00 fora
    }

    fn bh(day: i32, open: &str, close: &str, is_open: bool) -> BusinessHours {
        BusinessHours::new(uuid::Uuid::nil(), day, open.into(), close.into(), is_open)
    }

    #[test]
    fn loja_override_e_horario() {
        let hours = vec![bh(1, "08:00", "18:00", true)];
        assert!(is_store_open(&hours, "open", 3, 0)); // override aberto
        assert!(!is_store_open(&hours, "closed", 1, 600)); // override fechado
        assert!(is_store_open(&hours, "none", 1, 600)); // 10:00 aberto
        assert!(!is_store_open(&hours, "none", 1, 1200)); // 20:00 fechado
        assert!(is_store_open(&[], "none", 1, 600)); // sem horários → aberto
    }
}
