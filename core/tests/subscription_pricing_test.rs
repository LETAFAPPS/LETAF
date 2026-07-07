//! Testes do valor de cobrança por ciclo (`subscription::service::charge_amount`)
//! — preço da assinatura com desconto comercial (AI_RULES §11, §13). O backend
//! é a fonte da verdade do valor cobrado; a UI nunca calcula preço.

use rust_decimal_macros::dec;

use letaf_core::subscription::service::charge_amount;

#[test]
fn sem_desconto_cobra_o_valor_de_tabela() {
    assert_eq!(charge_amount(dec!(200.00), dec!(0), 1), dec!(200.00));
}

#[test]
fn desconto_incide_sobre_cada_mes_do_ciclo() {
    // Semestral: R$ 190/mês × 6 = 1140; desconto R$ 10/mês × 6 = 60.
    assert_eq!(charge_amount(dec!(1140.00), dec!(10.00), 6), dec!(1080.00));
}

#[test]
fn desconto_maior_que_o_bruto_zera_a_cobranca() {
    // Desconto absurdo nunca gera valor negativo.
    assert_eq!(charge_amount(dec!(200.00), dec!(500.00), 1), dec!(0));
}

#[test]
fn desconto_negativo_e_tratado_como_zero() {
    assert_eq!(charge_amount(dec!(200.00), dec!(-50.00), 1), dec!(200.00));
}

#[test]
fn desconto_exatamente_igual_ao_bruto_zera() {
    assert_eq!(charge_amount(dec!(180.00), dec!(15.00), 12), dec!(0));
}
