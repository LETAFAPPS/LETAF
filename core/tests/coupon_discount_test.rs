//! Testes da função pura `Coupon::discount_for` — trava o comportamento do
//! cálculo de desconto (anti-fraude: o servidor recomputa, nunca confia no
//! valor do frontend, AI_RULES §11). Rede de segurança para a futura migração
//! de dinheiro `f64 → Decimal` (docs/DEBITO_TECNICO_decimal.md): ao converter,
//! as asserções mantêm os MESMOS valores numéricos; se o cálculo preservar o
//! comportamento, os testes seguem verdes.

use uuid::Uuid;

use letaf_core::coupon::model::Coupon;

/// Cupom mínimo para exercitar `discount_for` (usa só type/kind/value/max).
fn coupon(coupon_type: &str, kind: &str, value: f64, max_discount: f64) -> Coupon {
    Coupon::new(
        Uuid::new_v4(),
        "T".into(),
        "CODE".into(),
        coupon_type.into(),
        kind.into(),
        value,
        0.0, // min_order_value — validado no service, não em discount_for
        max_discount,
        0,
        0,
        None,
        None,
    )
}

#[test]
fn fixed_discount_subtracts_value() {
    let c = coupon("standard", "fixed", 5.0, 0.0);
    assert_eq!(c.discount_for(100.0), 5.0);
}

#[test]
fn fixed_discount_never_exceeds_subtotal() {
    let c = coupon("standard", "fixed", 5.0, 0.0);
    assert_eq!(c.discount_for(3.0), 3.0);
}

#[test]
fn fixed_discount_on_zero_subtotal_is_zero() {
    let c = coupon("standard", "fixed", 5.0, 0.0);
    assert_eq!(c.discount_for(0.0), 0.0);
}

#[test]
fn percent_discount_applies_percentage() {
    let c = coupon("standard", "percent", 10.0, 0.0);
    assert_eq!(c.discount_for(100.0), 10.0);
}

#[test]
fn percent_discount_respects_max_cap() {
    let c = coupon("standard", "percent", 10.0, 7.0);
    assert_eq!(c.discount_for(100.0), 7.0);
}

#[test]
fn percent_over_100_is_clamped_to_subtotal() {
    // discount_value inválido (>100) é clampado em 100 → desconto = subtotal.
    let c = coupon("standard", "percent", 150.0, 0.0);
    assert_eq!(c.discount_for(80.0), 80.0);
}

#[test]
fn percent_negative_is_clamped_to_zero() {
    let c = coupon("standard", "percent", -5.0, 0.0);
    assert_eq!(c.discount_for(100.0), 0.0);
}

#[test]
fn free_shipping_gives_no_item_discount() {
    // free_shipping não desconta itens (desconto de frete é tratado à parte).
    let c = coupon("free_shipping", "fixed", 999.0, 0.0);
    assert_eq!(c.discount_for(100.0), 0.0);
}

#[test]
fn max_discount_zero_means_no_cap() {
    let c = coupon("standard", "percent", 50.0, 0.0);
    assert_eq!(c.discount_for(100.0), 50.0);
}
