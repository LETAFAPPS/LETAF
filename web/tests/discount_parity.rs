//! Paridade entre `web::discount` (f64, sobre o DTO `CatalogProduct`) e
//! `core::discount` (Decimal, sobre `Product`). A lógica é DUPLICADA de
//! propósito: reusar o core no cliente puxaria `rust_decimal` para o bundle
//! wasm. Este teste (dev-dependency, fora do bundle) trava a divergência —
//! se a regra de desconto mudar num lado e não no outro, quebra.

use letaf_core::discount as core_discount;
use letaf_core::product::model::{BalanceMode, Product};
use letaf_web::api::CatalogProduct;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use uuid::Uuid;

fn dec(f: f64) -> Decimal {
    Decimal::try_from(f).unwrap()
}

/// Produto do core com os mesmos parâmetros de desconto.
#[allow(clippy::too_many_arguments)]
fn core_product(
    price: f64,
    kind: Option<&str>,
    value: Option<f64>,
    min_qty: Option<f64>,
    tiers: Option<&str>,
) -> Product {
    Product::new(
        Uuid::nil(),
        "P".into(),
        None,
        None,
        None,
        Some(dec(price)),
        None,
        0.0,
        0.0,
        false,
        None,
        "un".into(),
        BalanceMode::default(),
        None,
        None,
        None,
        kind.map(String::from),
        value.map(dec),
        min_qty,
        tiers.map(String::from),
    )
}

/// Mesmo produto como DTO do catálogo (web). Construído por serde para não
/// depender da ordem dos campos de `CatalogProduct`.
fn web_product(
    price: f64,
    kind: Option<&str>,
    value: Option<f64>,
    min_qty: Option<f64>,
    tiers: Option<&str>,
) -> CatalogProduct {
    let mut obj = serde_json::json!({ "id": "p", "name": "P", "price": price });
    if let Some(k) = kind {
        obj["discount_kind"] = serde_json::json!(k);
    }
    if let Some(v) = value {
        obj["discount_value"] = serde_json::json!(v);
    }
    if let Some(m) = min_qty {
        obj["discount_min_qty"] = serde_json::json!(m);
    }
    if let Some(t) = tiers {
        obj["discount_tiers"] = serde_json::json!(t);
    }
    serde_json::from_value(obj).unwrap()
}

/// Compara os dois cálculos para o mesmo cenário, em várias quantidades.
fn assert_parity(
    price: f64,
    kind: Option<&str>,
    value: Option<f64>,
    min_qty: Option<f64>,
    tiers: Option<&str>,
) {
    let cp = core_product(price, kind, value, min_qty, tiers);
    let wp = web_product(price, kind, value, min_qty, tiers);
    for qty in [1.0, 2.0, 3.0, 5.0, 10.0] {
        let core = core_discount::effective_unit_price(&cp, qty).to_f64().unwrap();
        let web = letaf_web::discount::effective_unit_price(&wp, qty);
        assert!(
            (core - web).abs() < 0.005,
            "divergência em kind={kind:?} qty={qty}: core={core} web={web}",
        );
    }
}

#[test]
fn parity_sem_desconto() {
    assert_parity(10.0, None, None, None, None);
    assert_parity(10.0, Some("desconhecido"), Some(5.0), None, None);
}

#[test]
fn parity_fixed_e_percent() {
    assert_parity(10.0, Some("fixed"), Some(3.0), None, None);
    assert_parity(10.0, Some("fixed"), Some(15.0), None, None); // clamp em 0
    assert_parity(20.0, Some("percent"), Some(25.0), None, None);
    assert_parity(20.0, Some("percent"), Some(150.0), None, None); // clamp em 0
}

#[test]
fn parity_bulk_min_qty() {
    assert_parity(10.0, Some("bulk_fixed"), Some(2.0), Some(3.0), None);
    assert_parity(10.0, Some("bulk_percent"), Some(10.0), Some(5.0), None);
}

#[test]
fn parity_bulk_tiers() {
    let tiers = r#"[{"min_qty":3,"value":1.0},{"min_qty":6,"value":2.5}]"#;
    assert_parity(10.0, Some("bulk_fixed"), None, None, Some(tiers));
    let tiers_pct = r#"[{"min_qty":2,"value":5},{"min_qty":10,"value":20}]"#;
    assert_parity(10.0, Some("bulk_percent"), None, None, Some(tiers_pct));
}
