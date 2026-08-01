//! Testes do analytics de relatórios (core, domínio puro).
//!
//! Travam a regra de negócio extraída da UI (§1/§3): DRE, margens,
//! ticket médio, rankings e a janela de período — dinheiro exato em
//! `Decimal`, sem depender do relógio.

use chrono::{NaiveDate, NaiveTime};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use uuid::Uuid;

use letaf_core::category::model::Category;
use letaf_core::entity::BaseFields;
use letaf_core::order::model::{DeliveryType, Order, OrderItem, OrderStatus};
use letaf_core::product::model::{BalanceMode, Product};
use letaf_core::report::{self, ReportPeriod};

const CID: Uuid = Uuid::from_u128(0x1111_1111_1111_1111_1111_1111_1111_1111);

fn today() -> NaiveDate {
    // Quarta-feira.
    NaiveDate::from_ymd_opt(2026, 7, 15).unwrap()
}

/// Pedido posicionado às 12:00 LOCAIS (15:00 UTC) do dia informado.
fn order(total: Decimal, day: NaiveDate, status: OrderStatus) -> Order {
    order_at(total, day, 15, status)
}

/// Pedido numa hora UTC específica (a hora local é 3h antes — §6).
fn order_at(total: Decimal, day: NaiveDate, utc_hour: u32, status: OrderStatus) -> Order {
    let mut base = BaseFields::new(CID);
    base.created_at = day.and_time(NaiveTime::from_hms_opt(utc_hour, 0, 0).unwrap());
    base.updated_at = base.created_at;
    Order {
        base,
        customer_id: Uuid::nil(),
        number: 1,
        status,
        total,
        coupon_code: None,
        discount_amount: dec!(0),
        additional_amount: dec!(0),
        delivery_type: DeliveryType::Delivery,
        notes: None,
        cancellation_reason: None,
        payment_method: None,
        paid: true,
        items: vec![],
    }
}

fn item(product_id: Uuid, name: &str, quantity: f64, unit_price: Decimal) -> OrderItem {
    OrderItem {
        base: BaseFields::new(CID),
        order_id: Uuid::nil(),
        product_id,
        product_name: name.to_string(),
        quantity,
        unit_price,
        subtotal: unit_price * letaf_core::money::qty(quantity),
        notes: None,
        addons_json: None,
        list_unit_price: None,
    }
}

fn product(name: &str, cost: Option<Decimal>, category_id: Option<Uuid>) -> Product {
    Product::new(
        CID,
        name.to_string(),
        None,
        category_id,
        None,
        Some(dec!(10.00)),
        cost,
        0.0,
        0.0,
        true,
        None,
        "un".into(),
        BalanceMode::Weight,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
}

// ── Janela de período ────────────────────────────────────────────────────

#[test]
fn janela_da_semana_vai_de_segunda_a_domingo() {
    let w = report::period_window(today(), ReportPeriod::Weekly);
    assert_eq!(w.start, NaiveDate::from_ymd_opt(2026, 7, 13).unwrap());
    assert_eq!(w.end, NaiveDate::from_ymd_opt(2026, 7, 19).unwrap());
    // Período anterior: mesma semana deslocada 7 dias.
    assert_eq!(w.prev_start, NaiveDate::from_ymd_opt(2026, 7, 6).unwrap());
    assert_eq!(w.prev_end, NaiveDate::from_ymd_opt(2026, 7, 12).unwrap());
    assert_eq!(w.days, 7);
}

#[test]
fn janela_do_mes_cobre_o_mes_inteiro_mas_conta_dias_decorridos() {
    let w = report::period_window(today(), ReportPeriod::Monthly);
    assert_eq!(w.start, NaiveDate::from_ymd_opt(2026, 7, 1).unwrap());
    assert_eq!(w.end, NaiveDate::from_ymd_opt(2026, 7, 31).unwrap());
    assert_eq!(w.prev_start, NaiveDate::from_ymd_opt(2026, 6, 1).unwrap());
    assert_eq!(w.prev_end, NaiveDate::from_ymd_opt(2026, 6, 30).unwrap());
    assert_eq!(w.days, 15); // 1..15 de julho
}

#[test]
fn janela_do_dia_compara_com_ontem() {
    let w = report::period_window(today(), ReportPeriod::Daily);
    assert_eq!((w.start, w.end), (today(), today()));
    let ontem = NaiveDate::from_ymd_opt(2026, 7, 14).unwrap();
    assert_eq!((w.prev_start, w.prev_end), (ontem, ontem));
    assert_eq!(w.days, 1);
}

#[test]
fn janela_do_ano_vai_de_janeiro_ate_hoje_contra_o_ano_anterior() {
    let w = report::period_window(today(), ReportPeriod::Yearly);
    assert_eq!(w.start, NaiveDate::from_ymd_opt(2026, 1, 1).unwrap());
    assert_eq!(w.end, today());
    assert_eq!(w.prev_start, NaiveDate::from_ymd_opt(2025, 1, 1).unwrap());
    assert_eq!(w.prev_end, NaiveDate::from_ymd_opt(2025, 12, 31).unwrap());
}

#[test]
fn janela_do_mes_trata_a_virada_do_ano() {
    let jan = NaiveDate::from_ymd_opt(2026, 1, 10).unwrap();
    let w = report::period_window(jan, ReportPeriod::Monthly);
    assert_eq!(w.end, NaiveDate::from_ymd_opt(2026, 1, 31).unwrap());
    assert_eq!(w.prev_start, NaiveDate::from_ymd_opt(2025, 12, 1).unwrap());
    assert_eq!(w.prev_end, NaiveDate::from_ymd_opt(2025, 12, 31).unwrap());
}

// ── Filtros de base ──────────────────────────────────────────────────────

#[test]
fn janela_usa_o_fuso_da_loja_e_ignora_soft_delete() {
    let d = today();
    // 01:00 UTC do dia 16 = 22:00 LOCAIS do dia 15 → conta no dia 15.
    let noite = order_at(dec!(10), d.succ_opt().unwrap(), 1, OrderStatus::Delivered);
    let mut apagado = order(dec!(99), d, OrderStatus::Delivered);
    apagado.base.deleted_at = Some(apagado.base.created_at);
    let orders = vec![noite, apagado];

    let win = report::in_window(&orders, d, d);
    assert_eq!(win.len(), 1);
    assert_eq!(win[0].total, dec!(10));
}

#[test]
fn cancelados_saem_das_vendas_mas_ficam_no_total() {
    let d = today();
    let orders = vec![
        order(dec!(10), d, OrderStatus::Delivered),
        order(dec!(50), d, OrderStatus::Cancelled),
    ];
    let win = report::in_window(&orders, d, d);
    assert_eq!(win.len(), 2);
    assert_eq!(report::non_cancelled(&win).len(), 1);
}

// ── DRE ──────────────────────────────────────────────────────────────────

#[test]
fn dre_sem_custo_cadastrado_conta_custo_zero() {
    let d = today();
    let pid = Uuid::from_u128(7);
    let mut o = order(dec!(30.00), d, OrderStatus::Delivered);
    o.items = vec![item(pid, "Coxinha", 3.0, dec!(10.00))];
    let orders = [o];
    let refs: Vec<&Order> = orders.iter().collect();

    // Produto SEM `cost_price`: não há de onde tirar o custo.
    let m = report::financial(&refs, &[product("Coxinha", None, None)]);
    assert_eq!(m.revenue, dec!(30.00));
    assert_eq!(m.cost, Decimal::ZERO);
    assert_eq!(m.net, dec!(30.00));
    assert_eq!(m.margin_pct, dec!(100));
}

#[test]
fn dre_usa_custo_por_quantidade_e_calcula_margem() {
    let d = today();
    let pid = Uuid::from_u128(7);
    let mut o = order(dec!(40.00), d, OrderStatus::Delivered);
    o.items = vec![item(pid, "Coxinha", 4.0, dec!(10.00))];
    let orders = [o];
    let refs: Vec<&Order> = orders.iter().collect();

    let mut p = product("Coxinha", Some(dec!(2.50)), None);
    p.base.id = pid;
    let m = report::financial(&refs, &[p]);
    assert_eq!(m.cost, dec!(10.00)); // 2,50 × 4
    assert_eq!(m.net, dec!(30.00));
    assert_eq!(m.margin_pct, dec!(75));
    assert_eq!(m.avg_ticket, dec!(40.00));
}

#[test]
fn dre_sem_pedidos_nao_divide_por_zero() {
    let m = report::financial(&[], &[]);
    assert_eq!(m.revenue, Decimal::ZERO);
    // Margem com receita ZERO e ticket médio sem pedidos: ambos ZERO,
    // sem panic de divisão.
    assert_eq!(m.margin_pct, Decimal::ZERO);
    assert_eq!(m.avg_ticket, Decimal::ZERO);
    assert_eq!(m.orders_count, 0);
}

#[test]
fn margem_negativa_quando_o_custo_supera_a_receita() {
    let d = today();
    let pid = Uuid::from_u128(7);
    let mut o = order(dec!(10.00), d, OrderStatus::Delivered);
    o.items = vec![item(pid, "Coxinha", 1.0, dec!(10.00))];
    let orders = [o];
    let refs: Vec<&Order> = orders.iter().collect();
    let mut p = product("Coxinha", Some(dec!(15.00)), None);
    p.base.id = pid;

    let m = report::financial(&refs, &[p]);
    assert_eq!(m.net, dec!(-5.00));
    assert_eq!(m.margin_pct, dec!(-50));
}

#[test]
fn ticket_medio_divide_a_receita_pelos_pedidos_validos() {
    let d = today();
    let orders = [
        order(dec!(10.00), d, OrderStatus::Delivered),
        order(dec!(25.00), d, OrderStatus::Delivered),
    ];
    let refs: Vec<&Order> = orders.iter().collect();
    let m = report::financial(&refs, &[]);
    assert_eq!(m.avg_ticket, dec!(17.50));
}

#[test]
fn recebimentos_somam_so_as_formas_conhecidas() {
    let d = today();
    let mut pix = order(dec!(10.00), d, OrderStatus::Delivered);
    pix.payment_method = Some("pix".into());
    let mut cash = order(dec!(5.00), d, OrderStatus::Delivered);
    cash.payment_method = Some("cash".into());
    let mut carteira = order(dec!(99.00), d, OrderStatus::Delivered);
    carteira.payment_method = Some("wallet".into());
    let sem_forma = order(dec!(7.00), d, OrderStatus::Delivered);

    let orders = [pix, cash, carteira, sem_forma];
    let refs: Vec<&Order> = orders.iter().collect();
    let m = report::financial(&refs, &[]);
    // Receita bruta conta TODOS os pedidos válidos...
    assert_eq!(m.revenue, dec!(121.00));
    // ...mas o gauge só conhece dinheiro/PIX/crédito/débito.
    assert_eq!(m.methods.total(), dec!(15.00));
    assert_eq!(m.methods.pix, dec!(10.00));
    assert_eq!(m.methods.cash, dec!(5.00));
}

#[test]
fn fiado_em_aberto_ignora_quitados_e_cancelados() {
    let d = today();
    let mut aberto = order(dec!(30.00), d, OrderStatus::Delivered);
    aberto.payment_method = Some("wallet".into());
    aberto.paid = false;
    let mut quitado = order(dec!(80.00), d, OrderStatus::Delivered);
    quitado.payment_method = Some("wallet".into());
    quitado.paid = true;
    let mut cancelado = order(dec!(50.00), d, OrderStatus::Cancelled);
    cancelado.payment_method = Some("wallet".into());
    cancelado.paid = false;

    assert_eq!(
        report::outstanding_fiado(&[aberto, quitado, cancelado]),
        dec!(30.00)
    );
}

// ── Pedidos ──────────────────────────────────────────────────────────────

#[test]
fn metricas_de_pedidos_contam_cancelamento_canal_e_hora() {
    let d = today();
    let mut balcao = order(dec!(20.00), d, OrderStatus::Delivered);
    balcao.payment_method = Some("cash".into());
    let mut retirada = order_at(dec!(10.00), d, 23, OrderStatus::Ready);
    retirada.delivery_type = DeliveryType::Pickup;
    let entrega = order(dec!(10.00), d, OrderStatus::Delivered);
    let cancelado = order(dec!(99.00), d, OrderStatus::Cancelled);

    let all = [balcao, retirada, entrega, cancelado];
    let refs: Vec<&Order> = all.iter().collect();
    let valid = report::non_cancelled(&refs);
    let m = report::orders(&refs, &valid, &[]);

    assert_eq!((m.total, m.valid, m.cancelled), (4, 3, 1));
    assert_eq!(m.cancel_rate, dec!(25));
    assert_eq!(m.avg_ticket, dec!(40) / dec!(3)); // exato, sem erro de f64
    assert_eq!(m.channels.pdv, 1);
    assert_eq!(m.channels.delivery, 1);
    assert_eq!(m.channels.pickup, 1);
    // 15:00 UTC = 12h local; 23:00 UTC = 20h local (§6).
    assert_eq!(m.by_hour[12], 2);
    assert_eq!(m.by_hour[20], 1);
}

#[test]
fn taxa_de_cancelamento_sem_pedidos_e_zero() {
    let m = report::orders(&[], &[], &[]);
    assert_eq!(m.cancel_rate, Decimal::ZERO);
    assert_eq!(m.avg_ticket, Decimal::ZERO);
    assert_eq!(m.avg_prep_minutes, None);
    assert_eq!(m.completed_count, 0);
}

#[test]
fn tempo_medio_de_preparo_descarta_outliers() {
    let d = today();
    let mut normal = order(dec!(10.00), d, OrderStatus::Delivered);
    normal.base.updated_at = normal.base.created_at + chrono::Duration::minutes(20);
    // Fora da faixa 5s..6h → não entra na média.
    let mut esquecido = order(dec!(10.00), d, OrderStatus::Delivered);
    esquecido.base.updated_at = esquecido.base.created_at + chrono::Duration::hours(9);
    // Pendente não conta como completado.
    let pendente = order(dec!(10.00), d, OrderStatus::Pending);

    let all = [normal, esquecido, pendente];
    let refs: Vec<&Order> = all.iter().collect();
    let m = report::orders(&refs, &refs, &[]);
    assert_eq!(m.avg_prep_minutes, Some(20.0));
    assert_eq!(m.completed_count, 2);
}

// ── Produtos ─────────────────────────────────────────────────────────────

#[test]
fn produtos_rateiam_a_receita_do_pedido_entre_os_itens() {
    let d = today();
    let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
    // Total 90 com subtotal 100 (desconto de 10): o rateio distribui.
    let mut o = order(dec!(90.00), d, OrderStatus::Delivered);
    o.items = vec![
        item(a, "A", 2.0, dec!(25.00)), // linha 50
        item(b, "B", 1.0, dec!(50.00)), // linha 50
    ];
    let orders = [o];
    let refs: Vec<&Order> = orders.iter().collect();

    let mut pa = product("A", Some(dec!(5.00)), None);
    pa.base.id = a;
    let mut pb = product("B", None, None);
    pb.base.id = b;

    let m = report::products(&refs, &[pa, pb], &[]);
    assert_eq!(m.total_revenue, dec!(90.00));
    assert_eq!(m.total_units, 3.0);
    // Só A tem custo: 5,00 × 2.
    assert_eq!(m.total_cost, dec!(10.00));
    let esperado = (dec!(90.00) - dec!(10.00)) / dec!(90.00) * dec!(100);
    assert_eq!(m.margin_pct, esperado);
    // Mesma receita nos dois: empate desempata pelo NOME (determinismo).
    assert_eq!(m.ranking.len(), 2);
    assert_eq!(m.ranking[0].name, "A");
    assert_eq!(m.top_by_quantity.as_ref().unwrap().name, "A");
    assert_eq!(m.top_by_revenue.as_ref().unwrap().revenue, dec!(45.00));
}

#[test]
fn produto_sem_cadastro_usa_o_nome_do_item_e_categoria_vazia() {
    let d = today();
    let pid = Uuid::from_u128(9);
    let mut o = order(dec!(10.00), d, OrderStatus::Delivered);
    o.items = vec![item(pid, "Produto Excluído", 1.0, dec!(10.00))];
    let orders = [o];
    let refs: Vec<&Order> = orders.iter().collect();

    let m = report::products(&refs, &[], &[]);
    assert_eq!(m.ranking[0].name, "Produto Excluído");
    assert_eq!(m.ranking[0].category_name, "");
}

#[test]
fn produto_resolve_o_nome_da_categoria() {
    let d = today();
    let pid = Uuid::from_u128(9);
    let cat = Category::new(CID, "Salgados".into(), None);
    let mut p = product("Coxinha", None, Some(cat.base.id));
    p.base.id = pid;
    let mut o = order(dec!(10.00), d, OrderStatus::Delivered);
    o.items = vec![item(pid, "Coxinha", 1.0, dec!(10.00))];
    let orders = [o];
    let refs: Vec<&Order> = orders.iter().collect();

    let m = report::products(&refs, &[p], &[cat]);
    assert_eq!(m.ranking[0].category_name, "Salgados");
}

#[test]
fn produtos_sem_venda_nao_dividem_por_zero() {
    let m = report::products(&[], &[], &[]);
    assert_eq!(m.margin_pct, Decimal::ZERO);
    assert!(m.ranking.is_empty());
    assert!(m.top_by_revenue.is_none());
}

// ── Clientes ─────────────────────────────────────────────────────────────

#[test]
fn clientes_separam_novos_de_recorrentes_e_calculam_ltv() {
    let d = today();
    let (novo, antigo) = (Uuid::from_u128(11), Uuid::from_u128(12));
    let mes_passado = NaiveDate::from_ymd_opt(2026, 6, 10).unwrap();

    let mut o1 = order(dec!(100.00), d, OrderStatus::Delivered);
    o1.customer_id = novo;
    let mut o2 = order(dec!(20.00), d, OrderStatus::Delivered);
    o2.customer_id = antigo;
    let mut historico = order(dec!(30.00), mes_passado, OrderStatus::Delivered);
    historico.customer_id = antigo;
    // Pedido anônimo (balcão) não entra no LTV.
    let anonimo = order(dec!(999.00), d, OrderStatus::Delivered);

    let all = vec![o1, o2, historico, anonimo];
    let janela: Vec<&Order> = all.iter().take(2).collect();
    let m = report::customers(&janela, &all, d, d);

    assert_eq!(m.active_count, 2);
    assert_eq!(m.new_count, 1);
    assert_eq!(m.returning_count, 1);
    assert_eq!(m.return_rate, dec!(50));
    // LTV: novo = 100, antigo = 50 → média 75.
    assert_eq!(m.avg_ltv, dec!(75.00));
    assert_eq!(m.ranking.len(), 2);
    assert_eq!(m.ranking[0].revenue, dec!(100.00));
    assert_eq!(m.ranking[0].orders, 1);
    // VIP = LTV ≥ 2× a média (150) → ninguém aqui.
    assert!(!m.ranking[0].is_vip);
}

#[test]
fn cliente_com_ltv_dobrado_e_vip() {
    let d = today();
    let mut all = Vec::new();
    for (i, valor) in [dec!(300.00), dec!(10.00), dec!(10.00)].into_iter().enumerate() {
        let mut o = order(valor, d, OrderStatus::Delivered);
        o.customer_id = Uuid::from_u128(21 + i as u128);
        all.push(o);
    }
    let janela: Vec<&Order> = all.iter().collect();

    let m = report::customers(&janela, &all, d, d);
    // Média = 320 ÷ 3 ≈ 106,67 → VIP exige ≥ 213,33.
    assert!(m.ranking[0].is_vip);
    assert!(!m.ranking[1].is_vip);
    assert!(!m.ranking[2].is_vip);
}

#[test]
fn clientes_sem_pedidos_nao_dividem_por_zero() {
    let m = report::customers(&[], &[], today(), today());
    assert_eq!(m.return_rate, Decimal::ZERO);
    assert_eq!(m.avg_ltv, Decimal::ZERO);
    assert_eq!(m.active_count, 0);
}
