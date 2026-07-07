//! Utilitários de dinheiro em `Decimal` (AI_RULES §13 — exatidão financeira).
//!
//! Dinheiro no domínio é `rust_decimal::Decimal` (exato, sem erro de ponto
//! flutuante). Quantidades (peso/unidades) seguem `f64`; o produto
//! preço×quantidade converte a quantidade com [`qty`] para manter a conta
//! exata. Arredondamento monetário padrão em 2 casas via [`round2`].

use std::str::FromStr;

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

/// Lê um preço de um valor JSON de forma TOLERANTE: aceita tanto número
/// (formato legado, f64) quanto string decimal (formato canônico novo, sem
/// erro de ponto flutuante). Sempre arredonda a 2 casas. `None` se o valor
/// não for número nem string decimal válida.
///
/// Motivo (AI_RULES §13): preços dentro de blobs JSON (`variations`,
/// `discount_tiers`, `addons_json`) passam a ser gravados como STRING decimal
/// (exata); esta leitura tolerante mantém compatível o dado legado (número)
/// sem exigir migração em massa — o legado converte ao ser reescrito.
pub fn price_from_json(v: &serde_json::Value) -> Option<Decimal> {
    let d = match v {
        serde_json::Value::String(s) => Decimal::from_str(s.trim()).ok()?,
        serde_json::Value::Number(_) => Decimal::from_f64(v.as_f64()?)?,
        _ => return None,
    };
    Some(round2(d))
}

/// Forma canônica de um preço para gravar em JSON: string decimal com 2 casas
/// fixas (ex.: `"39.90"`, `"0.00"`). Sem `f64` no armazenamento. `{:.2}` do
/// `Decimal` preenche as casas (o `round2`/`to_string` não pad zeros à direita).
pub fn price_to_json_string(v: Decimal) -> String {
    format!("{:.2}", round2(v))
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
