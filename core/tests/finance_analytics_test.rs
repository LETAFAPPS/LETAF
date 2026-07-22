//! Testes do analytics financeiro (core, domínio puro). Travam a regra de
//! negócio extraída da UI (§3): KPIs, projeção de fluxo de caixa e agregação
//! do calendário — dinheiro exato em `Decimal`.

use chrono::{Duration, NaiveDate, NaiveTime};
use rust_decimal_macros::dec;
use uuid::Uuid;

use letaf_core::entity::BaseFields;
use letaf_core::finance::analytics;
use letaf_core::finance::model::{FinanceEntry, FinanceKind, FinanceStatus};
use letaf_core::order::model::{DeliveryType, Order, OrderStatus};

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 7, 15).unwrap()
}

fn entry(
    kind: FinanceKind,
    amount: rust_decimal::Decimal,
    due: NaiveDate,
    status: FinanceStatus,
) -> FinanceEntry {
    let mut e = FinanceEntry::new(Uuid::new_v4(), kind, "desc".into(), amount, due);
    e.status = status;
    e
}

fn pdv_order(total: rust_decimal::Decimal, day: NaiveDate, method: Option<&str>) -> Order {
    let mut base = BaseFields::new(Uuid::new_v4());
    base.created_at = day.and_time(NaiveTime::from_hms_opt(12, 0, 0).unwrap());
    Order {
        base,
        customer_id: Uuid::nil(),
        number: 1,
        status: OrderStatus::Delivered,
        total,
        coupon_code: None,
        discount_amount: dec!(0),
        additional_amount: dec!(0),
        delivery_type: DeliveryType::default(),
        notes: None,
        cancellation_reason: None,
        payment_method: method.map(|s| s.to_string()),
        items: vec![],
    }
}

#[test]
fn summary_agrega_abertas_e_vencidas() {
    let t = today();
    let entries = vec![
        entry(FinanceKind::Receivable, dec!(100.00), t + Duration::days(5), FinanceStatus::Pending),
        entry(FinanceKind::Receivable, dec!(50.00), t - Duration::days(3), FinanceStatus::Pending), // vencida
        entry(FinanceKind::Payable, dec!(30.00), t + Duration::days(2), FinanceStatus::Pending),
        entry(FinanceKind::Receivable, dec!(999.00), t, FinanceStatus::Received), // liquidada → ignora
        entry(FinanceKind::Payable, dec!(999.00), t, FinanceStatus::Cancelled),   // cancelada → ignora
    ];
    let s = analytics::summary(&entries, t);
    assert_eq!(s.to_receive, dec!(150.00));
    assert_eq!(s.to_pay, dec!(30.00));
    assert_eq!(s.count_receivable_open, 2);
    assert_eq!(s.count_payable_open, 1);
    assert_eq!(s.overdue, dec!(50.00));
    assert_eq!(s.overdue_count, 1);
    assert_eq!(s.expected_balance(), dec!(120.00)); // 150 - 30
}

#[test]
fn cash_flow_liquidado_no_paid_at_pendente_no_vencimento_e_pdv() {
    let t = today();
    // Payable pendente vence em t+1 → saída no dia 1.
    let payable = entry(FinanceKind::Payable, dec!(40.00), t + Duration::days(1), FinanceStatus::Pending);
    // Receivable liquidado: paid_at = t+2 → entrada no dia 2.
    let mut recv = entry(FinanceKind::Receivable, dec!(70.00), t - Duration::days(1), FinanceStatus::Received);
    recv.paid_at = Some((t + Duration::days(2)).and_time(NaiveTime::from_hms_opt(9, 0, 0).unwrap()));
    // Venda PDV paga em t → entrada no dia 0.
    let order = pdv_order(dec!(25.00), t, Some("pix"));
    // Venda sem método → não conta.
    let order2 = pdv_order(dec!(999.00), t, None);

    let flow = analytics::cash_flow(&[payable, recv], &[order, order2], t, 5);
    assert_eq!(flow.len(), 5);
    assert_eq!(flow[0].inflow, dec!(25.00)); // PDV
    assert_eq!(flow[0].outflow, dec!(0));
    assert_eq!(flow[1].outflow, dec!(40.00)); // payable pendente
    assert_eq!(flow[2].inflow, dec!(70.00)); // receivable liquidado no paid_at
    // Saldo cumulativo: d0=+25, d1=25-40=-15, d2=-15+70=55.
    assert_eq!(flow[0].balance, dec!(25.00));
    assert_eq!(flow[1].balance, dec!(-15.00));
    assert_eq!(flow[2].balance, dec!(55.00));
    assert_eq!(flow[4].balance, dec!(55.00)); // estável até o fim
}

#[test]
fn cash_flow_ignora_fora_da_janela_e_cancelados() {
    let t = today();
    let dentro = entry(FinanceKind::Payable, dec!(10.00), t + Duration::days(1), FinanceStatus::Pending);
    let fora = entry(FinanceKind::Payable, dec!(999.00), t + Duration::days(40), FinanceStatus::Pending);
    let cancelada = entry(FinanceKind::Payable, dec!(999.00), t, FinanceStatus::Cancelled);
    let flow = analytics::cash_flow(&[dentro, fora, cancelada], &[], t, 30);
    let total_out: rust_decimal::Decimal = flow.iter().map(|d| d.outflow).sum();
    assert_eq!(total_out, dec!(10.00));
}

#[test]
fn day_aggregates_agrupa_por_vencimento_e_ignora_cancelado_removido() {
    let t = today();
    let mut removida = entry(FinanceKind::Receivable, dec!(5.00), t, FinanceStatus::Pending);
    removida.base.deleted_at = Some(t.and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap()));
    let entries = vec![
        entry(FinanceKind::Receivable, dec!(100.00), t, FinanceStatus::Pending),
        entry(FinanceKind::Payable, dec!(30.00), t, FinanceStatus::Pending),
        entry(FinanceKind::Receivable, dec!(20.00), t + Duration::days(1), FinanceStatus::Pending),
        entry(FinanceKind::Payable, dec!(999.00), t, FinanceStatus::Cancelled), // ignora
        removida,                                                               // ignora
    ];
    let by_day = analytics::day_aggregates(&entries);
    let d0 = by_day.get(&t).unwrap();
    assert_eq!(d0.count, 2);
    assert_eq!(d0.inflow, dec!(100.00));
    assert_eq!(d0.outflow, dec!(30.00));
    assert_eq!(d0.net(), dec!(70.00));
    let d1 = by_day.get(&(t + Duration::days(1))).unwrap();
    assert_eq!(d1.count, 1);
    assert_eq!(d1.net(), dec!(20.00));
}
