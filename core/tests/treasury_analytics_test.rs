//! Testes da consolidação da tesouraria (core, domínio puro).
//!
//! Travam as regras extraídas da UI (§1/§3): o que é dinheiro NOVO
//! entrando/saindo do caixa, o corte da abertura da carteira, o recorte
//! do DIA no fuso da loja e o saldo derivado — tudo em `Decimal`.

use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use uuid::Uuid;

use letaf_core::entity::BaseFields;
use letaf_core::finance::model::{FinanceEntry, FinanceKind, FinanceStatus};
use letaf_core::finance::service::FIADO_AUTO_TAG;
use letaf_core::order::model::{DeliveryType, Order, OrderStatus};
use letaf_core::treasury::analytics::{
    self, CashDetail, CashMovement, CashSource, CashSources, TreasurySnapshot,
};
use letaf_core::treasury::model::{Treasury, TreasuryMovement, TreasuryMovementKind};
use letaf_core::wallet::model::{WalletMovement, WalletMovementKind};

const CID: Uuid = Uuid::from_u128(0x2222_2222_2222_2222_2222_2222_2222_2222);

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 7, 15).unwrap()
}

/// Instante UTC do dia de hoje (12:00 UTC = 09:00 na loja).
fn at(hour: u32) -> NaiveDateTime {
    today().and_time(NaiveTime::from_hms_opt(hour, 0, 0).unwrap())
}

/// Carteira aberta ANTES de tudo, com o saldo inicial informado.
fn treasury(initial: Decimal) -> Treasury {
    let mut t = Treasury::new(CID, initial, None);
    t.base.created_at = NaiveDate::from_ymd_opt(2026, 1, 1)
        .unwrap()
        .and_time(NaiveTime::MIN);
    t
}

fn order(total: Decimal, when: NaiveDateTime, method: Option<&str>, paid: bool) -> Order {
    let mut base = BaseFields::new(CID);
    base.created_at = when;
    base.updated_at = when;
    Order {
        base,
        customer_id: Uuid::nil(),
        number: 7,
        status: OrderStatus::Delivered,
        total,
        coupon_code: None,
        discount_amount: dec!(0),
        additional_amount: dec!(0),
        delivery_type: DeliveryType::Delivery,
        notes: None,
        cancellation_reason: None,
        payment_method: method.map(String::from),
        paid,
        items: vec![],
    }
}

fn wallet(kind: WalletMovementKind, amount: Decimal, when: NaiveDateTime) -> WalletMovement {
    let mut m = WalletMovement::new(CID, Uuid::from_u128(5), kind, amount, Decimal::ZERO);
    m.base.created_at = when;
    m
}

fn finance(
    kind: FinanceKind,
    status: FinanceStatus,
    amount: Decimal,
    when: NaiveDateTime,
) -> FinanceEntry {
    let mut e = FinanceEntry::new(CID, kind, "Lançamento".into(), amount, today());
    e.status = status;
    e.paid_at = Some(when);
    e
}

fn cashbox(kind: TreasuryMovementKind, amount: Decimal, when: NaiveDateTime) -> TreasuryMovement {
    let mut m = TreasuryMovement::new(CID, Uuid::from_u128(9), kind, amount, None);
    m.base.created_at = when;
    m
}

/// Consolida com as fontes informadas (o que não vier fica vazio).
fn consolidate(
    t: &Treasury,
    orders: &[Order],
    wallets: &[WalletMovement],
    entries: &[FinanceEntry],
    cash: &[TreasuryMovement],
) -> TreasurySnapshot {
    analytics::consolidate(
        &CashSources {
            treasury: t,
            orders,
            wallet_movements: wallets,
            finance_entries: entries,
            cashbox_movements: cash,
        },
        today(),
    )
}

// ── Pedidos ──────────────────────────────────────────────────────────────

#[test]
fn pedido_pago_entra_mas_carteira_cancelado_e_nao_pago_ficam_fora() {
    let t = treasury(dec!(100.00));
    let mut cancelado = order(dec!(50.00), at(12), Some("pix"), true);
    cancelado.status = OrderStatus::Cancelled;
    let orders = vec![
        order(dec!(30.00), at(12), Some("cash"), true),
        // Pago com carteira: consome crédito já depositado.
        order(dec!(99.00), at(12), Some("wallet"), true),
        // Ainda não pago.
        order(dec!(77.00), at(12), Some("pix"), false),
        cancelado,
    ];

    let s = consolidate(&t, &orders, &[], &[], &[]);
    assert_eq!(s.movements.len(), 1);
    assert_eq!(s.balance, dec!(130.00));
    assert_eq!(s.inflow, dec!(30.00));
    assert_eq!(s.breakdown.orders_in, dec!(30.00));
}

// ── Carteira de clientes ─────────────────────────────────────────────────

#[test]
fn carteira_conta_deposito_saque_e_ajuste_e_ignora_espelhos_contabeis() {
    let t = treasury(Decimal::ZERO);
    let movs = vec![
        wallet(WalletMovementKind::Deposit, dec!(100.00), at(12)),
        wallet(WalletMovementKind::Withdraw, dec!(40.00), at(12)),
        wallet(WalletMovementKind::ManualAdjust, dec!(10.00), at(12)),
        wallet(WalletMovementKind::ManualAdjust, dec!(-5.00), at(12)),
        // Espelhos contábeis — não são dinheiro entrando/saindo.
        wallet(WalletMovementKind::OrderCharge, dec!(1000.00), at(12)),
        wallet(WalletMovementKind::OrderRefund, dec!(1000.00), at(12)),
        wallet(WalletMovementKind::LimitChange, dec!(1000.00), at(12)),
        wallet(WalletMovementKind::ReceivableCharge, dec!(1000.00), at(12)),
        wallet(WalletMovementKind::ReceivableSettle, dec!(1000.00), at(12)),
    ];

    let s = consolidate(&t, &[], &movs, &[], &[]);
    assert_eq!(s.movements.len(), 4);
    assert_eq!(s.breakdown.wallet_in, dec!(110.00)); // 100 + 10
    assert_eq!(s.breakdown.wallet_out, dec!(45.00)); // 40 + 5
    assert_eq!(s.balance, dec!(65.00));
    // Ajuste negativo vira valor POSITIVO com direção de saída.
    let ajuste = s
        .movements
        .iter()
        .find(|m| m.detail == CashDetail::WalletAdjust && !m.positive)
        .expect("ajuste negativo");
    assert_eq!(ajuste.amount, dec!(5.00));
}

// ── Financeiro ───────────────────────────────────────────────────────────

#[test]
fn financeiro_conta_liquidados_e_ignora_o_fiado_automatico() {
    let t = treasury(Decimal::ZERO);
    let mut fiado = finance(
        FinanceKind::Receivable,
        FinanceStatus::Received,
        dec!(500.00),
        at(12),
    );
    fiado.notes = Some(FIADO_AUTO_TAG.to_string());
    let entries = vec![
        finance(FinanceKind::Receivable, FinanceStatus::Received, dec!(80.00), at(12)),
        finance(FinanceKind::Payable, FinanceStatus::Paid, dec!(20.00), at(12)),
        // Ainda em aberto: não movimentou caixa.
        finance(FinanceKind::Receivable, FinanceStatus::Pending, dec!(900.00), at(12)),
        finance(FinanceKind::Payable, FinanceStatus::Pending, dec!(900.00), at(12)),
        fiado,
    ];

    let s = consolidate(&t, &[], &[], &entries, &[]);
    assert_eq!(s.movements.len(), 2);
    assert_eq!(s.breakdown.finance_in, dec!(80.00));
    assert_eq!(s.breakdown.finance_out, dec!(20.00));
    assert_eq!(s.balance, dec!(60.00));
}

#[test]
fn lancamento_sem_paid_at_cai_no_updated_at() {
    let t = treasury(Decimal::ZERO);
    let mut e = finance(
        FinanceKind::Receivable,
        FinanceStatus::Received,
        dec!(15.00),
        at(12),
    );
    e.paid_at = None;
    e.base.updated_at = at(13);

    let s = consolidate(&t, &[], &[], &[e], &[]);
    assert_eq!(s.movements[0].at, at(13));
}

// ── Caixa (aportes/retiradas manuais) ────────────────────────────────────

#[test]
fn aportes_entram_e_retiradas_saem() {
    let t = treasury(dec!(10.00));
    let movs = vec![
        cashbox(TreasuryMovementKind::Deposit, dec!(200.00), at(12)),
        cashbox(TreasuryMovementKind::Withdraw, dec!(50.00), at(12)),
    ];

    let s = consolidate(&t, &[], &[], &[], &movs);
    assert_eq!(s.breakdown.cashbox_in, dec!(200.00));
    assert_eq!(s.breakdown.cashbox_out, dec!(50.00));
    assert_eq!(s.balance, dec!(160.00));
    assert_eq!(s.inflow - s.outflow, dec!(150.00));
}

// ── Corte da abertura e recorte do dia ───────────────────────────────────

#[test]
fn nada_anterior_a_abertura_da_carteira_entra() {
    let mut t = treasury(dec!(500.00));
    t.base.created_at = at(10);
    let orders = vec![
        // Antes da abertura: já está resumido no saldo inicial.
        order(dec!(999.00), at(9), Some("cash"), true),
        order(dec!(25.00), at(11), Some("cash"), true),
    ];

    let s = consolidate(&t, &orders, &[], &[], &[]);
    assert_eq!(s.movements.len(), 1);
    assert_eq!(s.balance, dec!(525.00));
}

#[test]
fn entradas_do_dia_usam_o_fuso_da_loja() {
    let t = treasury(Decimal::ZERO);
    // 01:00 UTC de hoje = 22:00 de ONTEM na loja → fora do dia.
    let ontem_a_noite = order(dec!(70.00), at(1), Some("pix"), true);
    // 12:00 UTC = 09:00 de hoje na loja → dentro do dia.
    let hoje = order(dec!(30.00), at(12), Some("pix"), true);

    let s = consolidate(&t, &[ontem_a_noite, hoje], &[], &[], &[]);
    // O SALDO usa todo o histórico...
    assert_eq!(s.balance, dec!(100.00));
    // ...mas entradas/saídas mostram só o DIA corrente.
    assert_eq!(s.inflow, dec!(30.00));
    assert_eq!(s.breakdown.orders_in, dec!(30.00));
}

#[test]
fn reserva_do_mes_e_o_liquido_desde_o_dia_primeiro() {
    let t = treasury(Decimal::ZERO);
    let mes_passado = NaiveDate::from_ymd_opt(2026, 6, 20)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(12, 0, 0).unwrap());
    let dia_1 = NaiveDate::from_ymd_opt(2026, 7, 1)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(12, 0, 0).unwrap());
    let orders = vec![
        order(dec!(1000.00), mes_passado, Some("cash"), true),
        order(dec!(400.00), dia_1, Some("cash"), true),
    ];
    let saidas = vec![cashbox(TreasuryMovementKind::Withdraw, dec!(100.00), at(12))];

    let s = consolidate(&t, &orders, &[], &[], &saidas);
    assert_eq!(s.reserved, dec!(300.00)); // 400 − 100 (só julho)
    assert_eq!(s.balance, dec!(1300.00));
}

#[test]
fn movimentos_saem_do_mais_recente_para_o_mais_antigo() {
    let t = treasury(Decimal::ZERO);
    let orders = vec![
        order(dec!(10.00), at(11), Some("cash"), true),
        order(dec!(20.00), at(14), Some("cash"), true),
        order(dec!(30.00), at(12), Some("cash"), true),
    ];
    let s = consolidate(&t, &orders, &[], &[], &[]);
    let ordem: Vec<Decimal> = s.movements.iter().map(|m| m.amount).collect();
    assert_eq!(ordem, vec![dec!(20.00), dec!(30.00), dec!(10.00)]);
    assert_eq!(s.movements[0].source(), CashSource::Order);
}

#[test]
fn carteira_sem_movimento_mantem_o_saldo_inicial() {
    let s = consolidate(&treasury(dec!(42.50)), &[], &[], &[], &[]);
    assert_eq!(s.balance, dec!(42.50));
    assert_eq!(s.inflow, Decimal::ZERO);
    assert_eq!(s.outflow, Decimal::ZERO);
    assert_eq!(s.reserved, Decimal::ZERO);
    assert!(s.movements.is_empty());
}

// ── Série por hora ───────────────────────────────────────────────────────

#[test]
fn serie_horaria_soma_por_hora_e_ignora_o_que_esta_fora_da_janela() {
    let movimentos = vec![
        movement(CashDetail::CashboxDeposit, dec!(50.00), true, at(15)),
        movement(CashDetail::CashboxWithdraw, dec!(20.00), false, at(15)),
        movement(CashDetail::CashboxDeposit, dec!(5.00), true, at(14)),
        // 12 horas antes do fim da janela → fora.
        movement(CashDetail::CashboxDeposit, dec!(999.00), true, at(3)),
    ];
    // Hora cheia atual na loja = 12:00 (15:00 UTC).
    let current = today().and_time(NaiveTime::from_hms_opt(12, 0, 0).unwrap());
    let flows = analytics::hourly_flow(&movimentos, current, 12);

    assert_eq!(flows.len(), 12);
    // Última fatia = hora corrente.
    let ultima = flows.last().unwrap();
    assert_eq!(ultima.start, current);
    assert_eq!(ultima.inflow, dec!(50.00));
    assert_eq!(ultima.outflow, dec!(20.00));
    assert_eq!(ultima.net(), dec!(30.00));
    // Hora anterior.
    assert_eq!(flows[10].inflow, dec!(5.00));
    // O movimento de 12h atrás não entrou em nenhuma fatia.
    let total: Decimal = flows.iter().map(|f| f.inflow).sum();
    assert_eq!(total, dec!(55.00));
}

fn movement(
    detail: CashDetail,
    amount: Decimal,
    positive: bool,
    at: NaiveDateTime,
) -> CashMovement {
    CashMovement { detail, description: None, at, amount, positive }
}

#[test]
fn serie_horaria_cobre_exatamente_a_janela_pedida() {
    let current = today().and_time(NaiveTime::from_hms_opt(12, 0, 0).unwrap());
    let flows = analytics::hourly_flow(&[], current, 12);
    assert_eq!(flows[0].start, current - Duration::hours(11));
    assert_eq!(flows[11].start, current);
}
