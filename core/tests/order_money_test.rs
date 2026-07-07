//! Testes do total de pedido (`order::service::order_total`) e do subtotal de
//! item (`OrderItem::new`) — regra de dinheiro do pedido (AI_RULES §11, §13).
//! O backend é a fonte da verdade do total; a UI nunca calcula preço.

use rust_decimal_macros::dec;
use uuid::Uuid;

use letaf_core::order::model::OrderItem;
use letaf_core::order::service::order_total;

#[test]
fn total_soma_itens_sem_desconto_nem_acrescimo() {
    let (discount, total) = order_total(dec!(100.00), dec!(0), dec!(0));
    assert_eq!(discount, dec!(0));
    assert_eq!(total, dec!(100.00));
}

#[test]
fn desconto_e_acrescimo_aplicados() {
    let (discount, total) = order_total(dec!(100.00), dec!(10.00), dec!(5.00));
    assert_eq!(discount, dec!(10.00));
    assert_eq!(total, dec!(95.00)); // 100 - 10 + 5
}

#[test]
fn desconto_maior_que_itens_e_clampado_ao_total_dos_itens() {
    // Cliente forja desconto absurdo: nunca pode zerar além dos itens.
    let (discount, total) = order_total(dec!(80.00), dec!(500.00), dec!(0));
    assert_eq!(discount, dec!(80.00), "desconto clampado ao total dos itens");
    assert_eq!(total, dec!(0.00), "total nunca fica negativo");
}

#[test]
fn desconto_negativo_e_tratado_como_zero() {
    let (discount, total) = order_total(dec!(50.00), dec!(-30.00), dec!(0));
    assert_eq!(discount, dec!(0));
    assert_eq!(total, dec!(50.00));
}

#[test]
fn acrescimo_negativo_e_tratado_como_zero() {
    let (_, total) = order_total(dec!(50.00), dec!(0), dec!(-20.00));
    assert_eq!(total, dec!(50.00));
}

#[test]
fn subtotal_do_item_e_quantidade_vezes_preco_arredondado() {
    let item = OrderItem::new(
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        "Coxinha".into(),
        3.0,
        dec!(4.50),
        None,
        None,
    );
    assert_eq!(item.subtotal, dec!(13.50)); // 3 × 4.50
}

#[test]
fn subtotal_arredonda_meio_centavo_para_cima() {
    // 1.5 × 3.33 = 4.995 → 5.00 (half-up, longe de zero)
    let item = OrderItem::new(
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        "Açaí (kg)".into(),
        1.5,
        dec!(3.33),
        None,
        None,
    );
    assert_eq!(item.subtotal, dec!(5.00));
}
