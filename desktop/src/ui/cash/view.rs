
use slint::{ModelRc, SharedString, VecModel};

use letaf_core::cash::model::{
    CashMovement, CashSession, MovementKind, SessionStatus, SessionSummary,
};

use crate::{
    CashMethodTotalRow, CashMovementRow, CashSessionRow, CashSummaryData, MainWindow,
};

use crate::format::{money_br as fmt_brl, money_br_signed as fmt_brl_signed};
use super::core::{fmt_duration, now_local, to_local};
use slint::ComponentHandle;
use crate::CashState;

pub(crate) fn apply_to_ui(
    ui: &MainWindow,
    active: Option<&CashSession>,
    recent: &[CashSession],
    movements: &[CashMovement],
    summary: &SessionSummary,
    suggested: f64,
) {
    // Summary
    let now = now_local();
    let (open, opened_summary, status_subtitle) = if let Some(s) = active {
        let opened_local = to_local(s.opened_at);
        let dur = fmt_duration(opened_local, now);
        let opened_str = opened_local.format("%H:%M").to_string();
        let date_str = opened_local.format("%d/%m/%Y").to_string();
        let summary_line = format!(
            "Aberto às {} · {} por {}",
            opened_str, date_str, s.operator_name
        );
        (true, summary_line, format!("há {}", dur))
    } else {
        let last_closed = recent.iter().find(|s| s.status == SessionStatus::Closed);
        let subtitle = match last_closed {
            Some(s) => {
                let closed = s.closed_at.map(to_local).unwrap_or_else(now_local);
                // Quem fechou (fallback para quem abriu, em sessões antigas
                // sem o registro de fechamento).
                let por = s.closed_operator_name.as_deref().unwrap_or(&s.operator_name);
                format!(
                    "Última sessão encerrada em {} às {} por {}",
                    closed.format("%d/%m"),
                    closed.format("%H:%M"),
                    por
                )
            }
            None => "Nenhuma sessão registrada ainda".into(),
        };
        (false, String::new(), subtitle)
    };

    let total_day = summary.sales_total;
    let cash_now = summary.cash_expected;
    let ticket_avg = if summary.sales_count > 0 {
        summary.sales_total / rust_decimal::Decimal::from(summary.sales_count)
    } else {
        rust_decimal::Decimal::ZERO
    };
    let total_expected = summary.by_method.values().map(|m| m.amount).sum::<rust_decimal::Decimal>();

    let s_data = CashSummaryData {
        open,
        status_headline: SharedString::from(if open { "Caixa Aberto" } else { "Caixa Fechado" }),
        status_subtitle: SharedString::from(status_subtitle),
        opened_summary: SharedString::from(opened_summary),
        total_day_display: SharedString::from(fmt_brl(total_day)),
        cash_now_display: SharedString::from(fmt_brl(cash_now)),
        sales_count_display: SharedString::from(summary.sales_count.to_string()),
        ticket_display: SharedString::from(fmt_brl(ticket_avg)),
        sangrias_display: SharedString::from(fmt_brl(summary.sangria_total)),
        suprimentos_display: SharedString::from(fmt_brl(summary.suprimento_total)),
        total_expected_display: SharedString::from(fmt_brl(total_expected)),
        session_id: SharedString::from(active.map(|s| s.base.id.to_string()).unwrap_or_default()),
        suggested_change_display: SharedString::from(fmt_brl(letaf_core::money::from_db_f64(suggested))),
        last_closed_summary: SharedString::default(),
    };
    ui.global::<CashState>().set_cash_summary(s_data);
    // Property bool dedicada para o modal de bloqueio do PDV — ver
    // comentário em main.slint sobre o panic "Recursion detected".
    ui.global::<CashState>().set_cash_blocked(!open);

    // Pré-popula sistema do modal "Fechar Caixa" — caso contrário os
    // campos abriam zerados independentemente da sessão atual.
    // - Dinheiro: saldo esperado da gaveta (cash_expected)
    // - Demais: total de vendas por método.
    let by = |k: &str| -> rust_decimal::Decimal {
        summary.by_method.get(k).map(|m| m.amount).unwrap_or(rust_decimal::Decimal::ZERO)
    };
    let sys_cash_val = summary.cash_expected;
    let sys_pix_val = by("pix");
    let sys_credit_val = by("credit");
    let sys_debit_val = by("debit");
    let sys_total_val = sys_cash_val + sys_pix_val + sys_credit_val + sys_debit_val;
    ui.global::<CashState>().set_cash_close_sys_cash(SharedString::from(fmt_brl(sys_cash_val)));
    ui.global::<CashState>().set_cash_close_sys_pix(SharedString::from(fmt_brl(sys_pix_val)));
    ui.global::<CashState>().set_cash_close_sys_credit(SharedString::from(fmt_brl(sys_credit_val)));
    ui.global::<CashState>().set_cash_close_sys_debit(SharedString::from(fmt_brl(sys_debit_val)));
    ui.global::<CashState>().set_cash_close_sys_total(SharedString::from(fmt_brl(sys_total_val)));
    // Sem nada informado ainda: a diferença fica em branco (só é
    // calculada depois que o operador preenche o campo).
    let pending = SharedString::from(super::PENDING_DIFF);
    ui.global::<CashState>().set_cash_close_diff_cash(pending.clone());
    ui.global::<CashState>().set_cash_close_diff_pix(pending.clone());
    ui.global::<CashState>().set_cash_close_diff_credit(pending.clone());
    ui.global::<CashState>().set_cash_close_diff_debit(pending.clone());
    ui.global::<CashState>().set_cash_close_diff_total(pending);
    ui.global::<CashState>().set_cash_close_in_total(SharedString::from(fmt_brl(rust_decimal::Decimal::ZERO)));
    ui.global::<CashState>().set_cash_close_has_diff(false);

    // Histórico
    let session_rows: Vec<CashSessionRow> = recent
        .iter()
        .map(|s| {
            let opened = to_local(s.opened_at);
            let closed_local = s.closed_at.map(to_local);
            let dur = match s.status {
                SessionStatus::Open => fmt_duration(opened, now),
                SessionStatus::Closed => fmt_duration(opened, closed_local.unwrap_or(opened)),
            };
            let diff_display = match (s.counted_cash, s.status.clone()) {
                (Some(counted), SessionStatus::Closed) => {
                    let diff = counted - s.initial_change;
                    fmt_brl_signed(diff)
                }
                _ => String::new(),
            };
            CashSessionRow {
                id: SharedString::from(s.base.id.to_string()),
                operator_name: SharedString::from(s.operator_name.clone()),
                closed_by: SharedString::from(s.closed_operator_name.clone().unwrap_or_default()),
                opened_date: SharedString::from(opened.format("%d/%m/%Y").to_string()),
                opened_time: SharedString::from(opened.format("%H:%M").to_string()),
                closed_time: SharedString::from(
                    closed_local
                        .map(|c| c.format("%H:%M").to_string())
                        .unwrap_or_default(),
                ),
                duration_display: SharedString::from(dur),
                total_display: SharedString::from(fmt_brl(s.initial_change)),
                difference_display: SharedString::from(diff_display),
                status: SharedString::from(s.status.to_string()),
                status_label: SharedString::from(match s.status {
                    SessionStatus::Open => "aberto",
                    SessionStatus::Closed => "fechado",
                }),
            }
        })
        .collect();
    ui.global::<CashState>().set_cash_sessions(ModelRc::new(VecModel::from(session_rows)));

    // Movimentos
    let mv_rows: Vec<CashMovementRow> = movements
        .iter()
        .rev()
        .map(|m| {
            let when = to_local(m.base.created_at);
            let (title, sign, method_label) = match m.kind {
                MovementKind::Sale => (
                    format!(
                        "Venda{}",
                        m.order_id
                            .map(|_| String::new())
                            .unwrap_or_default()
                    ),
                    "pos",
                    method_to_label(m.method.as_deref()),
                ),
                MovementKind::Sangria => (
                    format!("Sangria · {}", m.reason),
                    "neg",
                    "Dinheiro".into(),
                ),
                MovementKind::Suprimento => (
                    format!("Suprimento · {}", m.reason),
                    "pos",
                    // A forma REAL: recebimento de conta pode ter entrado
                    // por PIX/cartão e rotular tudo como "Dinheiro"
                    // contradizia o que a linha soma no fechamento.
                    method_to_label(m.method.as_deref()),
                ),
                MovementKind::SaleReversal => (
                    "Venda cancelada".to_string(),
                    "neg",
                    method_to_label(m.method.as_deref()),
                ),
                MovementKind::Opening => (
                    "Abertura".into(),
                    "pos",
                    "Dinheiro".into(),
                ),
            };
            let amount = if sign == "neg" { -m.amount } else { m.amount };
            CashMovementRow {
                id: SharedString::from(m.base.id.to_string()),
                time_display: SharedString::from(when.format("%H:%M").to_string()),
                kind: SharedString::from(m.kind.to_string()),
                title: SharedString::from(title),
                method_label: SharedString::from(method_label),
                amount_display: SharedString::from(fmt_brl_signed(amount)),
                sign: SharedString::from(sign),
            }
        })
        .collect();
    ui.global::<CashState>().set_cash_movements(ModelRc::new(VecModel::from(mv_rows)));

    // Totais por forma de pagamento — uma linha por forma no card
    // "Pagamentos" (rótulo, nº de transações e valor recebido).
    let methods = [
        ("cash", "Dinheiro", "money"),
        ("pix", "PIX", "pix"),
        ("credit", "Cartão Crédito", "credit"),
        ("debit", "Cartão Débito", "debit"),
        ("wallet", "Carteira", "wallet"),
    ];
    let mt_rows: Vec<CashMethodTotalRow> = methods
        .iter()
        .map(|(key, label, ui_key)| {
            let totals = summary.by_method.get(*key).copied().unwrap_or_default();
            CashMethodTotalRow {
                label: SharedString::from(*label),
                key: SharedString::from(*ui_key),
                count_display: SharedString::from(format!(
                    "{} {}",
                    totals.count,
                    if totals.count == 1 { "transação" } else { "transações" }
                )),
                amount_display: SharedString::from(fmt_brl(totals.amount)),
            }
        })
        .collect();
    ui.global::<CashState>().set_cash_method_totals(ModelRc::new(VecModel::from(mt_rows)));

    // Defaults dos campos do modal "Abrir caixa"
    ui.global::<CashState>().set_cash_open_operator(SharedString::from("Admin · Operador".to_string()));
    ui.global::<CashState>().set_cash_open_suggested(SharedString::from(fmt_brl(letaf_core::money::from_db_f64(suggested))));
}

pub(crate) fn method_to_label(method: Option<&str>) -> String {
    match method.unwrap_or("") {
        "cash" => "Dinheiro".into(),
        "pix" => "PIX".into(),
        "credit" => "Cartão Crédito".into(),
        "debit" => "Cartão Débito".into(),
        _ => "—".into(),
    }
}

