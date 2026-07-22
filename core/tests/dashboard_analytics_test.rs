//! Testes do analytics de dashboard (core, domínio puro). Travam a regra de
//! negócio que foi extraída da UT (§3): agregações determinísticas dada a lista
//! de pedidos e a data de referência.

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime};
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
    let m = dashboard::compute(&orders, t, DashboardPeriod::Week, 0);
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

    let m = dashboard::compute(&[cancelled, removed, ok], t, DashboardPeriod::Week, 0);
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
    let m = dashboard::compute(&orders, t, DashboardPeriod::Week, 0);
    // (150-100)/100 = +50%
    assert_eq!(m.revenue_today_delta, Some(50.0));
}

#[test]
fn delta_sem_base_e_none() {
    let c = Uuid::new_v4();
    let t = today();
    // Há receita hoje mas ZERO na base → sem base de comparação.
    let m = dashboard::compute(&[order(c, at(t, 10), dec!(10.00), "pix")], t, DashboardPeriod::Week, 0);
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
    let m = dashboard::compute(&[o], t, DashboardPeriod::Week, 0);
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
    let m = dashboard::compute(&orders, t, DashboardPeriod::Week, 0);
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
        let m = dashboard::compute(&orders, t, p, 0);
        assert_eq!(m.period_series.len(), 7, "período {:?} deveria ter 7 buckets", p);
    }
}

#[test]
fn vendas_da_semana_sao_segunda_a_domingo() {
    let t = today(); // quarta 2026-07-15
    let m = dashboard::compute(&[], t, DashboardPeriod::Week, 0);
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
    let m = dashboard::compute(&orders, t, DashboardPeriod::Week, 0);
    assert_eq!(m.period_best_day, Some(tuesday));
}

// ── Regressões da 6ª auditoria ────────────────────────────────────

#[test]
fn fuso_da_loja_move_venda_noturna_para_o_dia_local() {
    // `created_at` é UTC. Em BRT (-180 min), uma venda às 00:30 UTC de 16/07
    // aconteceu às 21:30 do dia 15 na loja — e deve contar no dia 15.
    let c = Uuid::new_v4();
    let dia15 = NaiveDate::from_ymd_opt(2026, 7, 15).unwrap();
    let dia16 = NaiveDate::from_ymd_opt(2026, 7, 16).unwrap();
    let venda = order(c, at(dia16, 0) + Duration::minutes(30), dec!(80.00), "pix");

    // Sem fuso (UTC): cai no dia 16.
    let utc = dashboard::compute(std::slice::from_ref(&venda), dia16, DashboardPeriod::Week, 0);
    assert_eq!(utc.revenue_today, dec!(80.00), "em UTC a venda pertence ao dia 16");

    // Com BRT: pertence ao dia 15, então "hoje = 16" não a contabiliza...
    let brt16 = dashboard::compute(std::slice::from_ref(&venda), dia16, DashboardPeriod::Week, -180);
    assert_eq!(brt16.revenue_today, dec!(0), "em BRT não é venda do dia 16");
    // ...e "hoje = 15" contabiliza.
    let brt15 = dashboard::compute(&[venda], dia15, DashboardPeriod::Week, -180);
    assert_eq!(brt15.revenue_today, dec!(80.00), "em BRT é venda do dia 15");
}

#[test]
fn comparativo_mensal_nao_duplica_ultimo_dia_de_mes_curto() {
    // Março tem 31 dias, fevereiro (2026) tem 28. Os dias 29/30/31 NÃO existem
    // em fevereiro: devem comparar contra ZERO, e não repetir 28/02.
    let c = Uuid::new_v4();
    let fev28 = NaiveDate::from_ymd_opt(2026, 2, 28).unwrap();
    let mar31 = NaiveDate::from_ymd_opt(2026, 3, 31).unwrap();
    let orders = vec![order(c, at(fev28, 12), dec!(100.00), "pix")];

    let m = dashboard::compute(&orders, mar31, DashboardPeriod::Month, 0);
    let com_valor: Vec<_> = m.compare.iter().filter(|p| p.previous > dec!(0)).collect();
    assert_eq!(com_valor.len(), 1, "28/02 deve aparecer UMA vez na série anterior");
    assert_eq!(com_valor[0].date.day(), 28);
}

#[test]
fn serie_mensal_nao_estoura_o_mes_nem_repete_rotulo() {
    // 30/04: os 7 buckets devem cobrir 1..=30 sem passar para maio e sem
    // repetir o dia inicial (antes o 7º bucket caía em 01/05, rotulado "1").
    let abr30 = NaiveDate::from_ymd_opt(2026, 4, 30).unwrap();
    let m = dashboard::compute(&[], abr30, DashboardPeriod::Month, 0);
    assert_eq!(m.period_series.len(), 7);
    for b in &m.period_series {
        assert_eq!(b.date.month(), 4, "bucket saiu de abril: {:?}", b.date);
    }
    let mut dias: Vec<u32> = m.period_series.iter().map(|b| b.date.day()).collect();
    let antes = dias.len();
    dias.sort_unstable();
    dias.dedup();
    assert_eq!(dias.len(), antes, "rótulos de dia duplicados: {dias:?}");
}

#[test]
fn melhor_dia_em_empate_e_deterministico() {
    // Dois dias com a MESMA receita → sempre o mais antigo (antes dependia da
    // ordem do HashMap e alternava entre refreshes).
    let c = Uuid::new_v4();
    let t = today();
    let seg = NaiveDate::from_ymd_opt(2026, 7, 13).unwrap();
    let ter = NaiveDate::from_ymd_opt(2026, 7, 14).unwrap();
    for _ in 0..8 {
        let orders = vec![
            order(c, at(seg, 10), dec!(300.00), "pix"),
            order(c, at(ter, 10), dec!(300.00), "pix"),
        ];
        let m = dashboard::compute(&orders, t, DashboardPeriod::Week, 0);
        assert_eq!(m.period_best_day, Some(seg), "empate deve resolver no dia mais antigo");
    }
}

#[test]
fn top_produtos_em_empate_ordena_por_nome() {
    let c = Uuid::new_v4();
    let t = today();
    for _ in 0..8 {
        let o = with_item(
            with_item(order(c, at(t, 10), dec!(0), "pix"), "Zebra", 1.0, dec!(50.00)),
            "Abacaxi",
            1.0,
            dec!(50.00),
        );
        let m = dashboard::compute(&[o], t, DashboardPeriod::Week, 0);
        assert_eq!(m.top_products[0].name, "Abacaxi", "empate deve ordenar por nome");
    }
}
