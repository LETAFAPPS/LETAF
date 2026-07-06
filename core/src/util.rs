//! Utilidades compartilhadas do domínio (AI_RULES §8/§14: sem
//! duplicação). Antes `round_2` (wallet) e `add_months` (finance e
//! subscription, com implementações divergentes) viviam espalhados.

use chrono::{Datelike, NaiveDate};

/// Arredondamento contábil padrão (2 casas) — evita drift de ponto
/// flutuante após muitas operações monetárias.
pub fn round_2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// Soma `months` (pode ser negativo) preservando o "fim de mês":
/// 31/01 + 1 → 28/02 (ou 29/02 em ano bissexto), 15/01 + 1 → 15/02.
/// Usa `div_euclid`/`rem_euclid` para lidar corretamente com meses
/// negativos. Fonte única para finance e subscription.
pub fn add_months(date: NaiveDate, months: i32) -> NaiveDate {
    let total = date.year() * 12 + date.month0() as i32 + months;
    let year = total.div_euclid(12);
    let month0 = total.rem_euclid(12) as u32;
    let day = date.day();
    // Recua até o último dia válido do mês alvo (d=1 sempre existe).
    for d in (1..=day).rev() {
        if let Some(out) = NaiveDate::from_ymd_opt(year, month0 + 1, d) {
            return out;
        }
    }
    date
}
