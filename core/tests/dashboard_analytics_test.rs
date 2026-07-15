//! Testes do analytics de dashboard (core, domínio puro). Travam a regra de
//! negócio que foi extraída da UT (§3): agregações determinísticas dada a lista
//! de pedidos e a data de referência.

use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime};
use rust_decimal_macros::dec;
use uuid::Uuid;

use letaf_core::dashboard::{self, DashboardPeriod};
use letaf_core::entity::BaseFields;
use letaf_core::order::model::{DeliveryType, Order, OrderItem, OrderStatus};

fn at(date: NaiveDate, hour: u32) -> NaiveDateTime {
    date.and_time(NaiveTime::from_hms_opt(hour, 0, 0).unwrap())
}

/// Cria um pedido válido com total, método e horário de criação dados.
fn order(company: Uuid, created: NaiveDateTime, total: rust_decimal::Decimal, method: &str) -> Order {
    let mut base = BaseFields::new(company);
    base.created_at = created;
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
        payment_method: Some(method.to_string()),
        items: vec![],
    }
}

fn with_item(mut o: Order, name: &str, qty: f64, subtotal: rust_decimal::Decimal) -> Order {
    o.items.push(OrderItem {
        base: BaseFields::new(o.base.company_id),
        order_id: o.base.id,
        product_id: Uuid::new_v4(),
        product_name: name.to_string(),
        quantity: qty,
        unit_price: subtotal,
        subtotal,
        notes: None,
        addons_json: None,
    });
    o
}

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 7, 15).unwrap() // uma quarta-feira
}

#[test]
fn receita_e_pedidos_de_hoje() {
    let c = Uuid::new_v4();
    let t = today();
    let orders = vec![
        order(c, at(t, 10), dec!(30.00), "pix"),
        order(c, at(t, 14), dec!(20.50), "cash"),
        order(c, at(t - Duration::days(1), 12), dec!(99.00), "pix"), // ontem, não conta
    ];
    let m = dashboard::compute(&orders, t, DashboardPeriod::Week);
    assert_eq!(m.revenue_today, dec!(50.50));
    assert_eq!(m.orders_today, 2);
    // ticket = 50,50 / 2 = 25,25 (exato em Decimal)
    assert_eq!(m.avg_ticket_today, dec!(25.25));
}

#[test]
fn pedido_cancelado_e_removido_sao_ignorados() {
    let c = Uuid::new_v4();
    let t = today();
    let mut cancelled = order(c, at(t, 10), dec!(100.00), "pix");
    cancelled.status = OrderStatus::Cancelled;
    let mut removed = order(c, at(t, 11), dec!(100.00), "pix");
    removed.base.deleted_at = Some(at(t, 12));
    let ok = order(c, at(t, 13), dec!(40.00), "pix");

    let m = dashboard::compute(&[cancelled, removed, ok], t, DashboardPeriod::Week);
    assert_eq!(m.revenue_today, dec!(40.00));
    assert_eq!(m.orders_today, 1);
}

#[test]
fn delta_percentual_vs_mesmo_dia_semana_anterior() {
    let c = Uuid::new_v4();
    let t = today();
    let last_week = t - Duration::days(7);
    let orders = vec![
        order(c, at(t, 10), dec!(150.00), "pix"),       // hoje: 150
        order(c, at(last_week, 10), dec!(100.00), "pix"), // base: 100
    ];
    let m = dashboard::compute(&orders, t, DashboardPeriod::Week);
    // (150-100)/100 = +50%
    assert_eq!(m.revenue_today_delta, Some(50.0));
}

#[test]
fn delta_sem_base_e_none() {
    let c = Uuid::new_v4();
    let t = today();
    // Há receita hoje mas ZERO na base → sem base de comparação.
    let m = dashboard::compute(&[order(c, at(t, 10), dec!(10.00), "pix")], t, DashboardPeriod::Week);
    assert_eq!(m.revenue_today_delta, None);
}

#[test]
fn top_produtos_ordenado_por_receita_e_top5() {
    let c = Uuid::new_v4();
    let t = today();
    // Dentro da janela da semana (hoje é quarta → seg..dom cobre hoje).
    let o = with_item(
        with_item(order(c, at(t, 10), dec!(0), "pix"), "Pizza", 2.0, dec!(60.00)),
        "Suco",
        3.0,
        dec!(15.00),
    );
    let m = dashboard::compute(&[o], t, DashboardPeriod::Week);
    assert_eq!(m.top_products.len(), 2);
    assert_eq!(m.top_products[0].name, "Pizza"); // maior receita primeiro
    assert_eq!(m.top_products[0].revenue, dec!(60.00));
    assert_eq!(m.top_products[0].quantity, 2.0);
    assert_eq!(m.top_products[1].name, "Suco");
}

#[test]
fn formas_de_pagamento_somam_por_metodo() {
    let c = Uuid::new_v4();
    let t = today();
    let orders = vec![
        order(c, at(t, 9), dec!(10.00), "pix"),
        order(c, at(t, 10), dec!(5.00), "pix"),
        order(c, at(t, 11), dec!(7.00), "credit"),
        order(c, at(t, 12), dec!(3.00), "cash"),
        order(c, at(t, 13), dec!(99.00), "wallet"), // fora do donut
    ];
    let m = dashboard::compute(&orders, t, DashboardPeriod::Week);
    assert_eq!(m.payments.pix, dec!(15.00));
    assert_eq!(m.payments.credit, dec!(7.00));
    assert_eq!(m.payments.debit, dec!(0));
    assert_eq!(m.payments.cash, dec!(3.00));
}

#[test]
fn series_do_periodo_tem_sempre_7_buckets() {
    let c = Uuid::new_v4();
    let t = today();
    let orders = vec![order(c, at(t, 10), dec!(20.00), "pix")];
    for p in [DashboardPeriod::Today, DashboardPeriod::Week, DashboardPeriod::Month] {
        let m = dashboard::compute(&orders, t, p);
        assert_eq!(m.period_series.len(), 7, "período {:?} deveria ter 7 buckets", p);
    }
}

#[test]
fn vendas_da_semana_sao_segunda_a_domingo() {
    let t = today(); // quarta 2026-07-15
    let m = dashboard::compute(&[], t, DashboardPeriod::Week);
    assert_eq!(m.sales_week.len(), 7);
    // Primeiro bucket = segunda (13/07/2026), último = domingo (19/07/2026).
    assert_eq!(m.sales_week[0].date, NaiveDate::from_ymd_opt(2026, 7, 13).unwrap());
    assert_eq!(m.sales_week[6].date, NaiveDate::from_ymd_opt(2026, 7, 19).unwrap());
}

#[test]
fn best_day_e_o_de_maior_receita_na_janela() {
    let c = Uuid::new_v4();
    let t = today();
    let monday = NaiveDate::from_ymd_opt(2026, 7, 13).unwrap();
    let tuesday = NaiveDate::from_ymd_opt(2026, 7, 14).unwrap();
    let orders = vec![
        order(c, at(monday, 10), dec!(50.00), "pix"),
        order(c, at(tuesday, 10), dec!(80.00), "pix"), // maior
        order(c, at(t, 10), dec!(30.00), "pix"),
    ];
    let m = dashboard::compute(&orders, t, DashboardPeriod::Week);
    assert_eq!(m.period_best_day, Some(tuesday));
}
