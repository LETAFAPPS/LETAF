//! Testes da função pura `Coupon::discount_for` — trava o comportamento do
//! cálculo de desconto (anti-fraude: o servidor recomputa, nunca confia no
//! valor do frontend, AI_RULES §11). Valores em `Decimal` (dinheiro exato).

use rust_decimal_macros::dec;
use uuid::Uuid;

use letaf_core::coupon::model::Coupon;

/// Cupom mínimo para exercitar `discount_for` (usa só type/kind/value/max).
fn coupon(coupon_type: &str, kind: &str, value: rust_decimal::Decimal, max_discount: rust_decimal::Decimal) -> Coupon {
    Coupon::new(
        Uuid::new_v4(),
        "T".into(),
        "CODE".into(),
        coupon_type.into(),
        kind.into(),
        value,
        dec!(0), // min_order_value — validado no service, não em discount_for
        max_discount,
        0,
        0,
        None,
        None,
    )
}

#[test]
fn fixed_discount_subtracts_value() {
    let c = coupon("standard", "fixed", dec!(5), dec!(0));
    assert_eq!(c.discount_for(dec!(100)), dec!(5));
}

#[test]
fn fixed_discount_never_exceeds_subtotal() {
    let c = coupon("standard", "fixed", dec!(5), dec!(0));
    assert_eq!(c.discount_for(dec!(3)), dec!(3));
}

#[test]
fn fixed_discount_on_zero_subtotal_is_zero() {
    let c = coupon("standard", "fixed", dec!(5), dec!(0));
    assert_eq!(c.discount_for(dec!(0)), dec!(0));
}

#[test]
fn percent_discount_applies_percentage() {
    let c = coupon("standard", "percent", dec!(10), dec!(0));
    assert_eq!(c.discount_for(dec!(100)), dec!(10));
}

#[test]
fn percent_discount_respects_max_cap() {
    let c = coupon("standard", "percent", dec!(10), dec!(7));
    assert_eq!(c.discount_for(dec!(100)), dec!(7));
}

#[test]
fn percent_over_100_is_clamped_to_subtotal() {
    let c = coupon("standard", "percent", dec!(150), dec!(0));
    assert_eq!(c.discount_for(dec!(80)), dec!(80));
}

#[test]
fn percent_negative_is_clamped_to_zero() {
    let c = coupon("standard", "percent", dec!(-5), dec!(0));
    assert_eq!(c.discount_for(dec!(100)), dec!(0));
}

#[test]
fn free_shipping_gives_no_item_discount() {
    let c = coupon("free_shipping", "fixed", dec!(999), dec!(0));
    assert_eq!(c.discount_for(dec!(100)), dec!(0));
}

#[test]
fn max_discount_zero_means_no_cap() {
    let c = coupon("standard", "percent", dec!(50), dec!(0));
    assert_eq!(c.discount_for(dec!(100)), dec!(50));
}

/// A normalização do código é a MESMA função exposta que o handler de
/// checkout usa para CONTAR usos. Se divergir, um código com espaço
/// (`"PROMO 10"`) casa o cupom mas conta 0 usos, furando `usage_limit` e
/// `per_user_limit` indefinidamente. Trava a forma canônica.
#[test]
fn normalize_code_remove_espacos_internos_e_maiusculiza() {
    use letaf_core::coupon::service::normalize_code;
    assert_eq!(normalize_code("PROMO 10"), "PROMO10");
    assert_eq!(normalize_code("  promo10  "), "PROMO10");
    assert_eq!(normalize_code("pro\tmo\u{00A0}10"), "PROMO10");
    assert_eq!(normalize_code("PROMO10"), "PROMO10");
    // Idempotente: normalizar o já-normalizado não muda nada — é o que
    // garante que a contagem (sobre o código gravado) case com o evaluate.
    assert_eq!(normalize_code(&normalize_code("p r o m o")), normalize_code("p r o m o"));
}
