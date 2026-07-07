//! Utilidades compartilhadas do domínio (AI_RULES §8/§14: sem
//! duplicação). `add_months` (finance e subscription, com implementações
//! divergentes) vivia espalhado; centralizado aqui. Dinheiro é `Decimal`
//! (ver `crate::money`) — não há arredondamento monetário em `f64`.

use chrono::{Datelike, NaiveDate};

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
