//! Disponibilidade por horário: produto (`availability_schedule`) e loja
//! (`business_hours`). Portado do web Dioxus. AI_RULES §1/§3/§8: funções
//! PURAS de exibição — recebem o "agora" `(dia, minutos)`; o backend é a
//! autoridade. No SSR não há relógio do navegador → `now = None` →
//! tratamos tudo como disponível/aberto (renderiza tudo, bom p/ SEO e
//! sem mismatch). Um `Effect` no cliente preenche o relógio na hidratação.

use leptos::prelude::RwSignal;
use serde::Deserialize;

use crate::api::BusinessHoursEntry;

const DAY_NAMES: [&str; 7] = [
    "Domingo", "Segunda", "Terça", "Quarta", "Quinta", "Sexta", "Sábado",
];

/// "Agora" do cliente: `(dia_da_semana 0=Dom..6=Sáb, minutos_desde_meia_noite)`.
/// `None` = desconhecido (SSR/antes da hidratação).
#[derive(Clone, Copy)]
pub struct Now(pub RwSignal<Option<(i32, i32)>>);

/// Lê o relógio do navegador (só no cliente). No SSR → `None`.
#[cfg(feature = "hydrate")]
pub fn browser_now() -> Option<(i32, i32)> {
    let d = js_sys::Date::new_0();
    Some((
        d.get_day() as i32,
        (d.get_hours() as i32) * 60 + d.get_minutes() as i32,
    ))
}

#[cfg(not(feature = "hydrate"))]
pub fn browser_now() -> Option<(i32, i32)> {
    None
}

#[derive(Deserialize)]
struct AvailabilityDay {
    day: i32,
    open: String,
    close: String,
    active: bool,
}

/// `true` se `mins` cai na janela `[open, close)`. Trata a VIRADA DE
/// MEIA-NOITE: quando `close <= open` a janela cruza 00:00 (ex.: 18:00→02:00),
/// valendo `mins >= open || mins < close`. `close == open` = 24h.
fn in_window(mins: i32, open: i32, close: i32) -> bool {
    if close > open {
        mins >= open && mins < close
    } else {
        mins >= open || mins < close
    }
}

/// Converte `"HH:MM"` em minutos desde meia-noite.
fn hhmm_to_minutes(s: &str) -> Option<i32> {
    let (h, m) = s.split_once(':')?;
    let h: i32 = h.parse().ok()?;
    let m: i32 = m.parse().ok()?;
    if !(0..24).contains(&h) || !(0..60).contains(&m) {
        return None;
    }
    Some(h * 60 + m)
}

/// `true` se o produto está vendável agora. `schedule` vazio/inválido →
/// disponível (degradação graciosa). `now = None` → disponível (SSR).
pub fn is_available_now(schedule: Option<&str>, now: Option<(i32, i32)>) -> bool {
    let Some(s) = schedule.filter(|s| !s.is_empty()) else {
        return true;
    };
    let Ok(entries) = serde_json::from_str::<Vec<AvailabilityDay>>(s) else {
        return true;
    };
    let Some((day, mins)) = now else {
        return true;
    };
    match entries.iter().find(|e| e.day == day) {
        Some(e) if e.active => {
            let open = hhmm_to_minutes(&e.open).unwrap_or(0);
            let close = hhmm_to_minutes(&e.close).unwrap_or(24 * 60);
            in_window(mins, open, close)
        }
        _ => false,
    }
}

/// `true` se a loja está aberta agora, respeitando o override
/// ("open"/"closed"/"none"). Sem horários ou `now=None` → aberto.
pub fn is_store_open_now(
    hours: &[BusinessHoursEntry],
    store_override: &str,
    now: Option<(i32, i32)>,
) -> bool {
    match store_override {
        "open" => return true,
        "closed" => return false,
        _ => {}
    }
    if hours.is_empty() {
        return true;
    }
    let Some((day, mins)) = now else {
        return true;
    };
    match hours.iter().find(|h| h.day_of_week == day) {
        Some(h) if h.is_open => {
            let open = hhmm_to_minutes(&h.open_time).unwrap_or(0);
            let close = hhmm_to_minutes(&h.close_time).unwrap_or(24 * 60);
            in_window(mins, open, close)
        }
        _ => false,
    }
}

/// Status da loja para o selo: `(aberta?, rótulo)`. `None` = não exibir
/// selo (sem horários cadastrados ou relógio ainda desconhecido no SSR).
pub fn store_status(
    hours: &[BusinessHoursEntry],
    store_override: &str,
    now: Option<(i32, i32)>,
) -> Option<(bool, String)> {
    if hours.is_empty() && store_override != "open" && store_override != "closed" {
        return None;
    }
    let (day, mins) = now?;
    let open = is_store_open_now(hours, store_override, Some((day, mins)));
    let today = hours.iter().find(|h| h.day_of_week == day);
    let label = if open {
        match (store_override, today) {
            ("open", _) => "Aberto".to_string(),
            (_, Some(h)) => format!("Aberto até {}h", h.close_time),
            _ => "Aberto agora".to_string(),
        }
    } else if store_override == "closed" {
        "Fechado".to_string()
    } else {
        match today {
            Some(h) if h.is_open && mins < hhmm_to_minutes(&h.open_time).unwrap_or(0) => {
                format!("Abre hoje às {}h", h.open_time)
            }
            _ => find_next_open_label(hours, day),
        }
    };
    Some((open, label))
}

/// Próximo dia aberto após `today` (offset 1 = amanhã).
fn find_next_open_label(hours: &[BusinessHoursEntry], today: i32) -> String {
    for offset in 1i32..=7 {
        let day = (today + offset).rem_euclid(7);
        if let Some(h) = hours.iter().find(|h| h.day_of_week == day && h.is_open) {
            if offset == 1 {
                return format!("Abre amanhã às {}h", h.open_time);
            }
            let name = DAY_NAMES.get(day as usize).copied().unwrap_or("?");
            return format!("Abre {} às {}h", name, h.open_time);
        }
    }
    "Fechado".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sched(day: i32, open: &str, close: &str, active: bool) -> String {
        format!(r#"[{{"day":{day},"open":"{open}","close":"{close}","active":{active}}}]"#)
    }
    fn bh(day: i32, open: &str, close: &str, is_open: bool) -> BusinessHoursEntry {
        BusinessHoursEntry {
            day_of_week: day,
            open_time: open.into(),
            close_time: close.into(),
            is_open,
        }
    }

    #[test]
    fn product_available_when_now_unknown_or_no_schedule() {
        assert!(is_available_now(Some(&sched(1, "08:00", "12:00", true)), None));
        assert!(is_available_now(None, Some((1, 600))));
        assert!(is_available_now(Some(""), Some((1, 600))));
        assert!(is_available_now(Some("lixo-invalido"), Some((1, 600))));
    }

    #[test]
    fn product_window_in_and_out() {
        let s = sched(1, "08:00", "12:00", true);
        assert!(is_available_now(Some(&s), Some((1, 600)))); // 10:00 dentro
        assert!(!is_available_now(Some(&s), Some((1, 60)))); // 01:00 fora
        assert!(!is_available_now(Some(&s), Some((2, 600)))); // outro dia
        let inactive = sched(1, "08:00", "12:00", false);
        assert!(!is_available_now(Some(&inactive), Some((1, 600)))); // dia inativo
    }

    #[test]
    fn product_janela_cruza_meia_noite() {
        // Bar noturno: abre 18:00 (1080) e fecha 02:00 (120) do dia seguinte.
        let s = sched(1, "18:00", "02:00", true);
        assert!(is_available_now(Some(&s), Some((1, 1140)))); // 19:00 dentro
        assert!(is_available_now(Some(&s), Some((1, 60)))); // 01:00 dentro (após meia-noite)
        assert!(!is_available_now(Some(&s), Some((1, 600)))); // 10:00 fora
        assert!(!is_available_now(Some(&s), Some((1, 150)))); // 02:30 fora
    }

    #[test]
    fn store_janela_cruza_meia_noite() {
        let hours = vec![bh(5, "18:00", "02:00", true)]; // sexta 18h→02h
        assert!(is_store_open_now(&hours, "none", Some((5, 1380)))); // 23:00 aberto
        assert!(is_store_open_now(&hours, "none", Some((5, 30)))); // 00:30 aberto
        assert!(!is_store_open_now(&hours, "none", Some((5, 720)))); // 12:00 fechado
    }

    #[test]
    fn store_override_and_hours() {
        let hours = vec![bh(1, "08:00", "18:00", true)];
        assert!(is_store_open_now(&hours, "open", Some((3, 0)))); // override aberto
        assert!(!is_store_open_now(&hours, "closed", Some((1, 600)))); // override fechado
        assert!(is_store_open_now(&hours, "none", Some((1, 600)))); // 10:00 aberto
        assert!(!is_store_open_now(&hours, "none", Some((1, 1200)))); // 20:00 fechado
        assert!(is_store_open_now(&[], "none", Some((1, 600)))); // sem horários → aberto
        assert!(is_store_open_now(&hours, "none", None)); // now desconhecido → aberto
    }

    #[test]
    fn store_status_labels() {
        let hours = vec![bh(1, "08:00", "18:00", true), bh(2, "08:00", "18:00", true)];
        let (open, label) = store_status(&hours, "none", Some((1, 600))).unwrap();
        assert!(open && label.contains("Aberto até 18:00"));
        let (open2, label2) = store_status(&hours, "none", Some((1, 360))).unwrap();
        assert!(!open2 && label2.contains("Abre hoje às 08:00"));
        // domingo (0) fechado → próximo aberto = segunda (amanhã)
        let (open3, label3) = store_status(&hours, "none", Some((0, 600))).unwrap();
        assert!(!open3 && label3.contains("Abre amanhã às 08:00"));
        assert!(store_status(&hours, "none", None).is_none()); // SSR → sem selo
        assert!(store_status(&[], "none", Some((1, 600))).is_none()); // sem horários → sem selo
    }
}
