//! Fuso horário do sistema — **America/São_Paulo (UTC−03:00)**.
//!
//! Regras (AI_RULES.md §1, §8, §11):
//! - Todo timestamp é gravado em **UTC** (`BaseFields::created_at`,
//!   `updated_at`, …). Este módulo é a ÚNICA porta de conversão para o
//!   horário da loja — nada no sistema deve usar `chrono::Local`, que
//!   depende do relógio do sistema operacional da máquina e faria a
//!   mesma venda cair em dias diferentes em cada computador.
//! - Offset FIXO: o Brasil não adota horário de verão desde 2019, então
//!   −03:00 vale o ano inteiro e dispensa base de dados de fuso.
//!
//! Use `now()`/`today()` para "agora"/"hoje" na loja e `to_local()`
//! para exibir um timestamp gravado em UTC.

use chrono::{Duration, FixedOffset, NaiveDate, NaiveDateTime, Timelike, Utc};

/// Offset da loja em minutos (−180 = UTC−03:00, horário de Brasília).
pub const OFFSET_MINUTES: i32 = -180;

/// O mesmo offset como `FixedOffset` do chrono, para quando for preciso
/// um `DateTime` com fuso (formatação com `%z`, RFC 3339, …).
pub fn offset() -> FixedOffset {
    FixedOffset::east_opt(OFFSET_MINUTES * 60).expect("offset fixo válido")
}

/// Converte um timestamp UTC (como gravado no banco) para o horário da
/// loja.
pub fn to_local(utc: NaiveDateTime) -> NaiveDateTime {
    utc + Duration::minutes(OFFSET_MINUTES as i64)
}

/// Converte um horário da loja de volta para UTC (para gravar/comparar
/// com o que está no banco).
pub fn to_utc(local: NaiveDateTime) -> NaiveDateTime {
    local - Duration::minutes(OFFSET_MINUTES as i64)
}

/// "Agora" no horário da loja.
pub fn now() -> NaiveDateTime {
    to_local(Utc::now().naive_utc())
}

/// "Hoje" no horário da loja.
pub fn today() -> NaiveDate {
    now().date()
}

/// Início da hora cheia atual no horário da loja (minutos e segundos
/// zerados) — base dos gráficos por hora.
pub fn current_hour() -> NaiveDateTime {
    let now = now();
    now.date()
        .and_hms_opt(now.hour(), 0, 0)
        .unwrap_or(now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converte_utc_para_o_horario_da_loja() {
        let utc = NaiveDate::from_ymd_opt(2026, 7, 31)
            .unwrap()
            .and_hms_opt(1, 3, 0)
            .unwrap();
        // 01:03 UTC = 22:03 do dia ANTERIOR em Brasília.
        let local = to_local(utc);
        assert_eq!(local.date(), NaiveDate::from_ymd_opt(2026, 7, 30).unwrap());
        assert_eq!(local.hour(), 22);
        assert_eq!(local.minute(), 3);
    }

    #[test]
    fn ida_e_volta_preserva_o_instante() {
        let utc = NaiveDate::from_ymd_opt(2026, 1, 5)
            .unwrap()
            .and_hms_opt(12, 34, 56)
            .unwrap();
        assert_eq!(to_utc(to_local(utc)), utc);
    }

    #[test]
    fn offset_bate_com_a_constante() {
        assert_eq!(offset().local_minus_utc(), OFFSET_MINUTES * 60);
    }
}
