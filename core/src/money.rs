//! Utilitários de dinheiro em `Decimal` (AI_RULES §13 — exatidão financeira).
//!
//! Dinheiro no domínio é `rust_decimal::Decimal` (exato, sem erro de ponto
//! flutuante). Quantidades (peso/unidades) seguem `f64`; o produto
//! preço×quantidade converte a quantidade com [`qty`] para manter a conta
//! exata. Arredondamento monetário padrão em 2 casas via [`round2`].

use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

/// Converte uma quantidade `f64` (pode ser fracionária, ex.: peso em kg) para
/// `Decimal`, para multiplicar por um preço sem perder exatidão. Valores não
/// finitos viram zero (defensivo — quantidade inválida não gera dinheiro).
pub fn qty(q: f64) -> Decimal {
    Decimal::from_f64(q).unwrap_or(Decimal::ZERO)
}

/// Arredonda um valor monetário para 2 casas decimais (padrão de exibição e
/// persistência). Half-up (meio centavo arredonda para cima) — esperado em
/// varejo.
pub fn round2(v: Decimal) -> Decimal {
    v.round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
}

/// Converte um `f64` lido do cache local (SQLite do desktop, que não tem tipo
/// decimal e guarda dinheiro como REAL) em `Decimal`, arredondando a 2 casas.
/// O round2 elimina o ruído de ponto flutuante do round-trip (ex.: 19.9899… →
/// 19.99). Cálculos e o servidor (NUMERIC) permanecem exatos; só o cache local
/// passa pelo `f64`.
pub fn from_db_f64(v: f64) -> Decimal {
    round2(Decimal::from_f64(v).unwrap_or(Decimal::ZERO))
}

/// Converte um valor em reais (`Decimal`) para centavos inteiros (`i64`) —
/// formato exigido pelas APIs de pagamento (Efi). Arredonda para o centavo.
pub fn to_cents(reais: Decimal) -> i64 {
    // Half-up (away-from-zero), consistente com `round2` — evita divergência
    // no meio-centavo entre exibição/persistência e o valor enviado à API.
    (reais * dec!(100))
        .round_dp_with_strategy(0, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
        .to_i64()
        .unwrap_or(0)
}
