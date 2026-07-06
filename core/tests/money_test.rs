//! Testes das primitivas de dinheiro (`core::money`) — base de toda a
//! aritmética financeira (AI_RULES §13). Travam arredondamento, conversão
//! para centavos e a limpeza do `f64` do cache local (desktop).

use rust_decimal_macros::dec;

use letaf_core::money::{from_db_f64, qty, round2, to_cents};

#[test]
fn round2_half_up_away_from_zero() {
    assert_eq!(round2(dec!(1.005)), dec!(1.01)); // meio centavo → cima
    assert_eq!(round2(dec!(2.994)), dec!(2.99));
    assert_eq!(round2(dec!(2.995)), dec!(3.00));
    assert_eq!(round2(dec!(-1.005)), dec!(-1.01)); // away from zero
    assert_eq!(round2(dec!(10)), dec!(10.00));
}

#[test]
fn to_cents_converts_reais_to_integer_cents() {
    assert_eq!(to_cents(dec!(19.99)), 1999);
    assert_eq!(to_cents(dec!(0)), 0);
    assert_eq!(to_cents(dec!(-5.00)), -500);
    assert_eq!(to_cents(dec!(1234.56)), 123456);
    // Arredonda para o centavo mais próximo.
    assert_eq!(to_cents(dec!(0.005)), 1);
}

#[test]
fn from_db_f64_cleans_float_noise() {
    // 19.99 em f64 não é exato; from_db_f64 + round2 normaliza para 19.99.
    assert_eq!(from_db_f64(19.99), dec!(19.99));
    // Clássico 0.1 + 0.2 = 0.30000000000000004 → 0.30.
    assert_eq!(from_db_f64(0.1 + 0.2), dec!(0.30));
    assert_eq!(from_db_f64(0.0), dec!(0));
    // Valor inválido (não-finito) vira zero (defensivo).
    assert_eq!(from_db_f64(f64::NAN), dec!(0));
    assert_eq!(from_db_f64(f64::INFINITY), dec!(0));
}

#[test]
fn qty_converts_quantity_to_decimal() {
    assert_eq!(qty(1.0), dec!(1));
    assert_eq!(qty(1.5), dec!(1.5));
    assert_eq!(qty(0.0), dec!(0));
    // NaN/infinito → zero (não gera dinheiro a partir de quantidade inválida).
    assert_eq!(qty(f64::NAN), dec!(0));
}

#[test]
fn price_times_quantity_is_exact() {
    // Preço × quantidade fracionária: exato via Decimal (peso, ex.: 1,5 kg).
    let unit = dec!(10.00);
    let subtotal = round2(qty(1.5) * unit);
    assert_eq!(subtotal, dec!(15.00));
    // Caso que erraria em f64: 0.1 × 3.
    let s = round2(qty(3.0) * dec!(0.10));
    assert_eq!(s, dec!(0.30));
}
