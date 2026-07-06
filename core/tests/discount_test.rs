//! Testes da função pura `discount::effective_unit_price`.
//!
//! Regras aplicadas (AI_RULES.md §11):
//! - A mesma função roda no backend (validar `unit_price` enviado pelo
//!   cliente) e no cliente web (decidir o que mostrar). Os testes
//!   blindam o contrato — se uma mudança no cálculo passar daqui sem
//!   atualizar o web/discount.rs, o anti-fraude começa a falhar com
//!   falsos positivos.

use uuid::Uuid;
use rust_decimal_macros::dec;

use letaf_core::discount::effective_unit_price;
use letaf_core::entity::BaseFields;
use letaf_core::product::model::{BalanceMode, Product};

fn make_product(price: rust_decimal::Decimal) -> Product {
    Product {
        base: BaseFields::new(Uuid::new_v4()),
        name: "X".into(),
        description: None,
        category_id: None,
        subcategory_id: None,
        price: Some(price),
        cost_price: None,
        stock_quantity: 0.0,
        min_stock: 0.0,
        unlimited_stock: false,
        barcode: None,
        unit: "un".into(),
        active: true,
        web_visible: true,
        balance_mode: BalanceMode::Weight,
        image_data: None,
        cover_color: None,
        availability_schedule: None,
        discount_kind: None,
        discount_value: None,
        discount_min_qty: None,
        discount_tiers: None,
        addon_group_ids: Vec::new(),
        variations: None,
    }
}

#[test]
fn no_discount_returns_base_price() {
    let p = make_product(dec!(10.0));
    assert_eq!(effective_unit_price(&p, 1.0), dec!(10.0));
    assert_eq!(effective_unit_price(&p, 100.0), dec!(10.0));
}

#[test]
fn fixed_discount_subtracts_value() {
    let mut p = make_product(dec!(10.0));
    p.discount_kind = Some("fixed".into());
    p.discount_value = Some(dec!(3.0));
    assert!((effective_unit_price(&p, 1.0) - dec!(7.0)).abs() < dec!(0.001));
}

#[test]
fn fixed_discount_never_below_zero() {
    let mut p = make_product(dec!(5.0));
    p.discount_kind = Some("fixed".into());
    p.discount_value = Some(dec!(20.0)); // desconto > preço
    assert_eq!(effective_unit_price(&p, 1.0), dec!(0.0));
}

#[test]
fn percent_discount_applies_correctly() {
    let mut p = make_product(dec!(100.0));
    p.discount_kind = Some("percent".into());
    p.discount_value = Some(dec!(20.0));
    assert!((effective_unit_price(&p, 1.0) - dec!(80.0)).abs() < dec!(0.001));
}

#[test]
fn bulk_fixed_single_tier_not_triggered_below_min() {
    let mut p = make_product(dec!(10.0));
    p.discount_kind = Some("bulk_fixed".into());
    p.discount_value = Some(dec!(2.0));
    p.discount_min_qty = Some(5.0);
    assert_eq!(effective_unit_price(&p, 4.0), dec!(10.0)); // abaixo do gatilho
    assert!((effective_unit_price(&p, 5.0) - dec!(8.0)).abs() < dec!(0.001)); // exatamente
    assert!((effective_unit_price(&p, 10.0) - dec!(8.0)).abs() < dec!(0.001)); // acima
}

#[test]
fn bulk_percent_single_tier() {
    let mut p = make_product(dec!(100.0));
    p.discount_kind = Some("bulk_percent".into());
    p.discount_value = Some(dec!(10.0));
    p.discount_min_qty = Some(3.0);
    assert_eq!(effective_unit_price(&p, 2.0), dec!(100.0));
    assert!((effective_unit_price(&p, 3.0) - dec!(90.0)).abs() < dec!(0.001));
}

#[test]
fn bulk_fixed_multi_tiers_pick_highest_satisfied() {
    let mut p = make_product(dec!(10.0));
    p.discount_kind = Some("bulk_fixed".into());
    p.discount_tiers = Some(
        r#"[{"min_qty":2,"value":1},{"min_qty":5,"value":2},{"min_qty":10,"value":4}]"#.into()
    );
    assert_eq!(effective_unit_price(&p, 1.0), dec!(10.0));
    assert!((effective_unit_price(&p, 2.0) - dec!(9.0)).abs() < dec!(0.001));
    assert!((effective_unit_price(&p, 4.0) - dec!(9.0)).abs() < dec!(0.001));
    assert!((effective_unit_price(&p, 5.0) - dec!(8.0)).abs() < dec!(0.001));
    assert!((effective_unit_price(&p, 9.0) - dec!(8.0)).abs() < dec!(0.001));
    assert!((effective_unit_price(&p, 10.0) - dec!(6.0)).abs() < dec!(0.001));
    assert!((effective_unit_price(&p, 50.0) - dec!(6.0)).abs() < dec!(0.001));
}

#[test]
fn bulk_tiers_in_unsorted_order_still_picks_highest() {
    let mut p = make_product(dec!(10.0));
    p.discount_kind = Some("bulk_fixed".into());
    // JSON na "ordem errada" — a função deve ordenar internamente.
    p.discount_tiers = Some(
        r#"[{"min_qty":10,"value":4},{"min_qty":2,"value":1},{"min_qty":5,"value":2}]"#.into()
    );
    assert!((effective_unit_price(&p, 6.0) - dec!(8.0)).abs() < dec!(0.001));
}

#[test]
fn invalid_discount_kind_falls_back_to_base() {
    let mut p = make_product(dec!(10.0));
    p.discount_kind = Some("mystery".into());
    p.discount_value = Some(dec!(5.0));
    assert_eq!(effective_unit_price(&p, 1.0), dec!(10.0));
}

#[test]
fn malformed_tiers_json_falls_back_to_base() {
    let mut p = make_product(dec!(10.0));
    p.discount_kind = Some("bulk_fixed".into());
    p.discount_tiers = Some("not valid json".into());
    assert_eq!(effective_unit_price(&p, 100.0), dec!(10.0));
}
