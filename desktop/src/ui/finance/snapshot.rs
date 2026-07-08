
use chrono::{Datelike, Duration, Local, NaiveDate};
use rust_decimal::prelude::ToPrimitive;
use slint::{Color, ModelRc, SharedString, VecModel};
use uuid::Uuid;

use letaf_core::finance::model::{
    FinanceEntry, FinanceKind, FinanceStatus,
};
use letaf_core::finance_category::model::FinanceCategory;
use letaf_core::order::model::{Order, OrderStatus};

fn money_br(v: f64) -> String {
    crate::format::money_br_f64(v)
}
use crate::{
    FinanceCalendarCell, FinanceCashFlowPoint, FinanceCategoryOption, FinanceDayRow,
    FinanceEntryRow, FinanceKpi, FinanceTab, MainWindow,
};

use super::state::CalState;
use super::helpers::{balance_tone, days_label, kpi, kpi_ex, money_signed, parse_hex_color, payment_method_label, tab};

// ── Snapshot ────────────────────────────────────────────────────

/// Snapshot Send-safe enviado entre tokio task e event loop.
/// Filtragem por aba/busca acontece em `apply_snapshot` (lá tem
/// acesso ao estado da UI — `get_finance_active_tab`/`search_query`).
pub(crate) struct Snapshot {
    kpis: Vec<FinanceKpi>,
    tabs: Vec<FinanceTab>,
    entries: Vec<EntryRaw>,
    categories: Vec<CategoryRaw>,
    cash_flow: Vec<CashFlowRaw>,
    cash_flow_summary: String,
    // ── Calendário ──
    cal_title: String,
    cal_month_total: String,
    cal_cells: Vec<CalendarCellRaw>,
    cal_detail_header: String,
    cal_detail_summary: String,
    cal_detail_rows: Vec<DayRowRaw>,
    cal_selected_key: String, // "AAAA-MM-DD" ou ""
}

#[derive(Clone)]
pub(crate) struct CalendarCellRaw {
    day: i32,
    in_month: bool,
    count: i32,
    total_display: String,
    total_tone: String,
    today: bool,
    selected: bool,
}

#[derive(Clone)]
pub(crate) struct DayRowRaw {
    id: String,
    title: String,
    subtitle: String,
    amount_display: String,
    amount_tone: String,
    status_label: String,
    status: String,
    can_settle: bool,
    kind: String,
}

/// Versão Send-safe do ponto de fluxo de caixa.
#[derive(Clone)]
pub(crate) struct CashFlowRaw {
    label: String,
    inflow_progress: f32,
    outflow_progress: f32,
    balance_progress: f32,
    tooltip: String,
    today: bool,
}

#[derive(Clone)]
pub(crate) struct EntryRaw {
    id: String,
    kind: String,
    description: String,
    party_name: String,
    category_name: String,
    category_color: Color,
    amount_display: String,
    amount_color: Color,
    due_date_display: String,
    days_display: String,
    status: String,
    status_label: String,
    status_color: Color,
    payment_method_label: String,
    action_label: String,
    installment_label: String,
    // Valor numérico (com sinal) — usado p/ somar o total do grupo.
    amount: f64,
    // Cabeçalho de grupo (preenchido no apply, na 1ª entry do grupo).
    group_first: bool,
    group_label: String,
    group_meta: String,
    group_color: Color,
    // Paginação por grupo (preenchida no apply, ao fatiar em janelas de 10).
    group_last: bool,
    group_page: i32,
    group_pages: i32,
    group_status: String,
}

#[derive(Clone)]
pub(crate) struct CategoryRaw {
    id: String,
    name: String,
    color: String,
    scope: String,
}

pub(crate) fn build_snapshot(
    entries: &[FinanceEntry],
    categories: &[FinanceCategory],
    orders: &[Order],
    cal: &CalState,
) -> Snapshot {
    let today = Local::now().date_naive();

    // KPIs agregados em cima do conjunto inteiro (não-filtrado por aba/busca).
    let mut to_receive = 0.0_f64;
    let mut to_pay = 0.0_f64;
    let mut overdue = 0.0_f64;
    let mut overdue_count = 0_i64;
    let mut count_receivable_open = 0_i64;
    let mut count_payable_open = 0_i64;
    let mut total_receivable_open = 0.0_f64;
    let mut total_payable_open = 0.0_f64;

    for e in entries {
        if e.status == FinanceStatus::Cancelled || e.status.is_settled() {
            continue;
        }
        match e.kind {
            FinanceKind::Receivable => {
                to_receive += e.amount.to_f64().unwrap_or(0.0);
                count_receivable_open += 1;
                total_receivable_open += e.amount.to_f64().unwrap_or(0.0);
            }
            FinanceKind::Payable => {
                to_pay += e.amount.to_f64().unwrap_or(0.0);
                count_payable_open += 1;
                total_payable_open += e.amount.to_f64().unwrap_or(0.0);
            }
        }
        if e.is_overdue(today) {
            overdue += e.amount.to_f64().unwrap_or(0.0);
            overdue_count += 1;
        }
    }
    let expected_balance = to_receive - to_pay;

    // SALDO PREVISTO e VENCIDOS neutros (text-primary, seguem o tema);
    // VENCIDOS é o 2º card. A cor passada é ignorada (value_neutral).
    let neutral = Color::from_rgb_u8(0x1A, 0x1A, 0x1A);
    let kpis = vec![
        kpi_ex(
            "SALDO PREVISTO",
            &money_signed(expected_balance),
            "Saldo Final",
            neutral,
            balance_tone(expected_balance),
            true,
        ),
        kpi_ex(
            "VENCIDOS",
            &money_br(overdue),
            &format!("{} Contas Atrasadas", overdue_count),
            // Valor em laranja (não neutro) — destaca contas vencidas.
            Color::from_rgb_u8(0xE8, 0x73, 0x1C),
            if overdue > 0.0 { "neg" } else { "neutral" },
            false,
        ),
        kpi(
            "A RECEBER",
            &money_br(to_receive),
            &format!("{} Contas Abertas", count_receivable_open),
            Color::from_rgb_u8(0x2E, 0x7D, 0x32),
            "pos",
        ),
        kpi(
            "A PAGAR",
            &money_br(to_pay),
            &format!("{} Contas Abertas", count_payable_open),
            Color::from_rgb_u8(0xE5, 0x39, 0x35),
            "neg",
        ),
    ];

    let tabs = vec![
        tab(
            "receivable",
            "A Receber",
            count_receivable_open,
            total_receivable_open,
        ),
        tab(
            "payable",
            "A Pagar",
            count_payable_open,
            total_payable_open,
        ),
    ];

    // Lista crua (filtragem por aba/busca acontece no apply via UI props).
    let cat_by_id: std::collections::HashMap<Uuid, &FinanceCategory> =
        categories.iter().map(|c| (c.base.id, c)).collect();
    let entries_raw: Vec<EntryRaw> = entries
        .iter()
        .map(|e| build_entry_row(e, &cat_by_id, today))
        .collect();

    let categories_raw = categories
        .iter()
        .map(|c| CategoryRaw {
            id: c.base.id.to_string(),
            name: c.name.clone(),
            color: c.color.clone(),
            scope: c.scope.to_string(),
        })
        .collect();

    let (cash_flow, cash_flow_summary) = build_cash_flow(entries, orders, today, 30);
    let cal_data = build_calendar(entries, today, cal);

    Snapshot {
        kpis,
        tabs,
        entries: entries_raw,
        categories: categories_raw,
        cash_flow,
        cash_flow_summary,
        cal_title: cal_data.title,
        cal_month_total: cal_data.month_total,
        cal_cells: cal_data.cells,
        cal_detail_header: cal_data.detail_header,
        cal_detail_summary: cal_data.detail_summary,
        cal_detail_rows: cal_data.detail_rows,
        cal_selected_key: cal_data.selected_key,
    }
}

/// Saída agregada do builder do calendário (separada para legibilidade).
pub(crate) struct CalendarBundle {
    title: String,
    month_total: String,
    cells: Vec<CalendarCellRaw>,
    detail_header: String,
    detail_summary: String,
    detail_rows: Vec<DayRowRaw>,
    selected_key: String,
}

pub(crate) fn build_calendar(entries: &[FinanceEntry], today: NaiveDate, cal: &CalState) -> CalendarBundle {
    let year = cal.year;
    let month = cal.month;
    let first_of_month = NaiveDate::from_ymd_opt(year, month, 1)
        .unwrap_or_else(|| Local::now().date_naive());
    let days_in_month = days_in_month(year, month) as i64;
    // Início do grid = domingo da semana que contém o dia 1.
    let weekday_of_first = first_of_month.weekday();
    let offset = weekday_of_first.num_days_from_sunday() as i64;
    let grid_start = first_of_month - Duration::days(offset);

    let mut by_day: std::collections::HashMap<NaiveDate, (i64, f64, f64)> =
        std::collections::HashMap::new();
    for e in entries {
        if e.status == FinanceStatus::Cancelled || e.base.deleted_at.is_some() {
            continue;
        }
        let entry = by_day.entry(e.due_date).or_insert((0, 0.0, 0.0));
        entry.0 += 1;
        match e.kind {
            FinanceKind::Receivable => entry.1 += e.amount.to_f64().unwrap_or(0.0),
            FinanceKind::Payable => entry.2 += e.amount.to_f64().unwrap_or(0.0),
        }
    }

    let cells: Vec<CalendarCellRaw> = (0..42)
        .map(|i| {
            let d = grid_start + Duration::days(i as i64);
            let in_month = d.month() == month && d.year() == year;
            let agg = by_day.get(&d).copied().unwrap_or((0, 0.0, 0.0));
            let (count, inflow, outflow) = agg;
            let net = inflow - outflow;
            let (total_display, total_tone) = if count == 0 {
                (String::new(), "neutral".to_string())
            } else if net > 0.0 {
                (format!("+{}", money_br(net)), "pos".into())
            } else if net < 0.0 {
                (format!("−{}", money_br(-net)), "neg".into())
            } else {
                (money_br(0.0), "neutral".into())
            };
            let selected = cal.selected_day == Some(d.day())
                && in_month;
            CalendarCellRaw {
                day: d.day() as i32,
                in_month,
                count: count as i32,
                total_display,
                total_tone,
                today: d == today,
                selected,
            }
        })
        .collect();

    // Total do mês = soma de receivables − payables com vencimento no mês.
    let month_net: f64 = by_day
        .iter()
        .filter(|(d, _)| d.year() == year && d.month() == month)
        .map(|(_, (_, inflow, outflow))| inflow - outflow)
        .sum();
    let month_total = money_signed(month_net);
    let title = format!("{} · {}", month_pt(month), year);

    // Detalhe do dia selecionado.
    let selected_date = cal
        .selected_day
        .and_then(|d| NaiveDate::from_ymd_opt(year, month, d));
    let mut detail_rows = Vec::new();
    let mut detail_header = String::new();
    let mut detail_summary = String::new();
    let mut selected_key = String::new();
    if let Some(sel) = selected_date {
        selected_key = sel.format("%Y-%m-%d").to_string();
        let is_today = sel == today;
        detail_header = if is_today {
            format!("HOJE · DIA {}", sel.format("%d/%m"))
        } else {
            format!("DIA {}", sel.format("%d/%m/%Y"))
        };
        let day_entries: Vec<&FinanceEntry> = entries
            .iter()
            .filter(|e| {
                e.status != FinanceStatus::Cancelled
                    && e.base.deleted_at.is_none()
                    && e.due_date == sel
            })
            .collect();
        let inflow: f64 = day_entries
            .iter()
            .filter(|e| matches!(e.kind, FinanceKind::Receivable))
            .map(|e| e.amount.to_f64().unwrap_or(0.0))
            .sum();
        let outflow: f64 = day_entries
            .iter()
            .filter(|e| matches!(e.kind, FinanceKind::Payable))
            .map(|e| e.amount.to_f64().unwrap_or(0.0))
            .sum();
        let net = inflow - outflow;
        detail_summary = if day_entries.is_empty() {
            "Sem Lançamentos".into()
        } else {
            format!("{} Lançamentos · {}", day_entries.len(), money_signed(net))
        };

        for e in day_entries {
            let amount_tone = match e.kind {
                FinanceKind::Receivable => "pos",
                FinanceKind::Payable => "neg",
            };
            let amount_display = match e.kind {
                FinanceKind::Receivable => format!("+{}", money_br(e.amount.to_f64().unwrap_or(0.0))),
                FinanceKind::Payable => format!("−{}", money_br(e.amount.to_f64().unwrap_or(0.0))),
            };
            let is_overdue = e.is_overdue(today);
            let status_label = if is_overdue {
                "Vencido".into()
            } else {
                match e.status {
                    FinanceStatus::Pending => "Pendente".to_string(),
                    FinanceStatus::Scheduled => "Agendado".into(),
                    FinanceStatus::Paid => "Pago".into(),
                    FinanceStatus::Received => "Recebido".into(),
                    FinanceStatus::Cancelled => "Cancelado".into(),
                }
            };
            let status = if is_overdue {
                "overdue".to_string()
            } else {
                e.status.to_string()
            };
            detail_rows.push(DayRowRaw {
                id: e.base.id.to_string(),
                title: e.description.clone(),
                subtitle: e.party_name.clone(),
                amount_display,
                amount_tone: amount_tone.into(),
                status_label,
                status,
                can_settle: !e.status.is_settled()
                    && e.status != FinanceStatus::Cancelled,
                kind: e.kind.to_string(),
            });
        }
    }

    // `days_in_month` é informativo para os logs; pra evitar warning,
    // referencia o valor (cobre garantia de range correto se mudar de
    // estratégia de geração do grid no futuro).
    let _ = days_in_month;

    CalendarBundle {
        title,
        month_total,
        cells,
        detail_header,
        detail_summary,
        detail_rows,
        selected_key,
    }
}

pub(crate) fn days_in_month(year: i32, month: u32) -> u32 {
    let (ny, nm) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    let first_next = NaiveDate::from_ymd_opt(ny, nm, 1).unwrap();
    let last_this = first_next - Duration::days(1);
    last_this.day()
}

pub(crate) fn month_pt(m: u32) -> &'static str {
    match m {
        1 => "Janeiro", 2 => "Fevereiro", 3 => "Março", 4 => "Abril",
        5 => "Maio", 6 => "Junho", 7 => "Julho", 8 => "Agosto",
        9 => "Setembro", 10 => "Outubro", 11 => "Novembro", 12 => "Dezembro",
        _ => "—",
    }
}

/// Calcula o gráfico de fluxo de caixa para os próximos `days` dias.
///
/// Composição:
/// - **Entradas** (verde): receivables liquidados no dia +
///   vendas (`Order`) pagas com `payment_method` preenchido no dia
///   (decisão do usuário: PDV conta como entrada).
/// - **Saídas** (vermelho): payables liquidados no dia OU pendentes
///   com `due_date` no dia (estimativa de saída futura).
///
/// O `today` da janela é o primeiro dia; vamos até `today + days - 1`.
/// Para dias futuros, payables pendentes contam como saída esperada.
pub(crate) fn build_cash_flow(
    entries: &[FinanceEntry],
    orders: &[Order],
    today: NaiveDate,
    days: i64,
) -> (Vec<CashFlowRaw>, String) {
    let span = days.max(1) as usize;
    let mut inflows = vec![0.0_f64; span];
    let mut outflows = vec![0.0_f64; span];

    let in_range = |d: NaiveDate| -> Option<usize> {
        let i = (d - today).num_days();
        if (0..span as i64).contains(&i) {
            Some(i as usize)
        } else {
            None
        }
    };

    for e in entries {
        if e.status == FinanceStatus::Cancelled {
            continue;
        }
        // Liquidado → conta no dia da baixa.
        if e.status.is_settled() {
            if let Some(paid) = e.paid_at {
                if let Some(idx) = in_range(paid.date()) {
                    match e.kind {
                        FinanceKind::Receivable => inflows[idx] += e.amount.to_f64().unwrap_or(0.0),
                        FinanceKind::Payable => outflows[idx] += e.amount.to_f64().unwrap_or(0.0),
                    }
                }
            }
        } else {
            // Pendente/agendado → previsão na due_date.
            if let Some(idx) = in_range(e.due_date) {
                match e.kind {
                    FinanceKind::Receivable => inflows[idx] += e.amount.to_f64().unwrap_or(0.0),
                    FinanceKind::Payable => outflows[idx] += e.amount.to_f64().unwrap_or(0.0),
                }
            }
        }
    }

    // Vendas pagas do PDV — entrada no dia do pedido.
    for o in orders {
        if o.base.deleted_at.is_some() || o.status == OrderStatus::Cancelled {
            continue;
        }
        if o.payment_method.is_none() {
            continue;
        }
        let day = o.base.created_at.date();
        if let Some(idx) = in_range(day) {
            inflows[idx] += o.total.to_f64().unwrap_or(0.0);
        }
    }

    // Saldo cumulativo a partir de 0 (caixa começa zerado para fins
    // de gráfico relativo; o "saldo previsto" absoluto vai no header).
    let mut balance = vec![0.0_f64; span];
    let mut acc = 0.0_f64;
    for i in 0..span {
        acc += inflows[i] - outflows[i];
        balance[i] = acc;
    }

    let max_flow = inflows
        .iter()
        .chain(outflows.iter())
        .copied()
        .fold(0.0_f64, f64::max);
    let min_balance = balance.iter().copied().fold(0.0_f64, f64::min);
    let max_balance = balance.iter().copied().fold(0.0_f64, f64::max);
    let balance_range = (max_balance - min_balance).max(0.001);

    let points = (0..span)
        .map(|i| {
            let day = today + Duration::days(i as i64);
            let inflow = inflows[i];
            let outflow = outflows[i];
            let bal = balance[i];
            let label = if i == 0 || (i + 1) % 7 == 0 {
                format!("d{}", i + 1)
            } else {
                String::new()
            };
            let tooltip = if inflow > 0.0 || outflow > 0.0 {
                format!(
                    "{} · Entradas {} · Saídas {} · Saldo {}",
                    day.format("%d/%m"),
                    money_br(inflow),
                    money_signed(-outflow),
                    money_signed(bal),
                )
            } else {
                format!("{} · sem movimentos", day.format("%d/%m"))
            };
            CashFlowRaw {
                label,
                inflow_progress: if max_flow > 0.0 {
                    (inflow / max_flow) as f32
                } else {
                    0.0
                },
                outflow_progress: if max_flow > 0.0 {
                    (outflow / max_flow) as f32
                } else {
                    0.0
                },
                balance_progress: ((bal - min_balance) / balance_range) as f32,
                tooltip,
                today: i == 0,
            }
        })
        .collect();

    let summary = format!(
        "Projeção com base nos lançamentos atuais · saldo previsto {}",
        money_signed(balance[span - 1]),
    );
    (points, summary)
}

pub(crate) fn build_entry_row(
    e: &FinanceEntry,
    cat_by_id: &std::collections::HashMap<Uuid, &FinanceCategory>,
    today: NaiveDate,
) -> EntryRaw {
    let is_overdue = e.is_overdue(today);
    let (status_label, status_color) = match e.status {
        _ if is_overdue => ("Vencido".to_string(), Color::from_rgb_u8(0xC6, 0x28, 0x28)),
        FinanceStatus::Pending => ("Pendente".into(), Color::from_rgb_u8(0xE8, 0x73, 0x1C)),
        FinanceStatus::Scheduled => ("Agendado".into(), Color::from_rgb_u8(0x1E, 0x88, 0xE5)),
        FinanceStatus::Paid => ("Pago".into(), Color::from_rgb_u8(0x2E, 0x7D, 0x32)),
        FinanceStatus::Received => ("Recebido".into(), Color::from_rgb_u8(0x2E, 0x7D, 0x32)),
        FinanceStatus::Cancelled => ("Cancelado".into(), Color::from_rgb_u8(0x9E, 0x9E, 0x9E)),
    };

    // Cor normal do valor pela natureza da conta — inclusive quando
    // vencida: A Receber (verde) / A Pagar (vermelho); baixado = verde.
    let amount_color = if e.status.is_settled() || matches!(e.kind, FinanceKind::Receivable) {
        Color::from_rgb_u8(0x2E, 0x7D, 0x32)
    } else {
        Color::from_rgb_u8(0xE5, 0x39, 0x35)
    };

    let amount_display = match e.kind {
        FinanceKind::Receivable => format!("+{}", money_br(e.amount.to_f64().unwrap_or(0.0))),
        FinanceKind::Payable => format!("−{}", money_br(e.amount.to_f64().unwrap_or(0.0))),
    };

    let (cat_name, cat_color) = e
        .category_id
        .and_then(|id| cat_by_id.get(&id))
        .map(|c| (c.name.clone(), parse_hex_color(&c.color)))
        .unwrap_or_else(|| (String::new(), Color::from_rgb_u8(0xBD, 0xBD, 0xBD)));

    let due_date_display = e.due_date.format("%d/%m/%Y").to_string();
    let days_display = days_label(today, e.due_date, e.status.is_settled());

    let action_label = if e.status.is_settled() || e.status == FinanceStatus::Cancelled {
        String::new()
    } else {
        match e.kind {
            FinanceKind::Payable => "Pagar".into(),
            FinanceKind::Receivable => "Receber".into(),
        }
    };

    let installment_label = if e.installment_total > 1 {
        format!("{}/{}", e.installment_index, e.installment_total)
    } else {
        String::new()
    };

    let payment_method_label = e
        .payment_method
        .as_deref()
        .map(payment_method_label)
        .unwrap_or_default();

    EntryRaw {
        id: e.base.id.to_string(),
        kind: e.kind.to_string(),
        description: e.description.clone(),
        party_name: e.party_name.clone(),
        category_name: cat_name,
        category_color: cat_color,
        amount_display,
        amount_color,
        due_date_display,
        days_display,
        status: if is_overdue {
            "overdue".to_string()
        } else {
            e.status.to_string()
        },
        status_label,
        status_color,
        payment_method_label,
        action_label,
        installment_label,
        amount: e.amount.to_f64().unwrap_or(0.0),
        group_first: false,
        group_label: String::new(),
        group_meta: String::new(),
        group_color: Color::from_rgb_u8(0x9E, 0x9E, 0x9E),
        group_last: false,
        group_page: 1,
        group_pages: 1,
        group_status: String::new(),
    }
}

/// Quantidade de contas exibidas por página, por grupo de status.
const FINANCE_PAGE_SIZE: usize = 10;

/// Página atual de cada grupo de status. Autoridade no backend (§11);
/// o frontend apenas dispara a troca via `finance-set-group-page`.
#[derive(Clone, Copy)]
pub(crate) struct GroupPages {
    pub(crate) overdue: i32,
    pub(crate) pending: i32,
    pub(crate) paid: i32,
    pub(crate) cancelled: i32,
}

impl GroupPages {
    fn get(&self, key: &str) -> i32 {
        match key {
            "overdue" => self.overdue,
            "pending" => self.pending,
            "paid" => self.paid,
            _ => self.cancelled,
        }
    }
    fn set(&mut self, key: &str, v: i32) {
        match key {
            "overdue" => self.overdue = v,
            "pending" => self.pending = v,
            "paid" => self.paid = v,
            _ => self.cancelled = v,
        }
    }
}

fn group_rank(status: &str) -> u8 {
    match status {
        "overdue" => 0,
        "pending" | "scheduled" => 1,
        "paid" | "received" => 2,
        _ => 3, // cancelled / outros
    }
}

/// Chave normalizada do grupo (usada p/ rotear a paginação).
fn group_key(status: &str) -> &'static str {
    match status {
        "overdue" => "overdue",
        "pending" | "scheduled" => "pending",
        "paid" | "received" => "paid",
        _ => "cancelled",
    }
}

fn group_label(status: &str) -> (&'static str, Color) {
    match status {
        "overdue" => ("VENCIDOS", Color::from_rgb_u8(0xC6, 0x28, 0x28)),
        "pending" | "scheduled" => ("PENDENTES", Color::from_rgb_u8(0xE8, 0x73, 0x1C)),
        "paid" | "received" => ("LIQUIDADOS", Color::from_rgb_u8(0x2E, 0x7D, 0x32)),
        _ => ("CANCELADOS", Color::from_rgb_u8(0x9E, 0x9E, 0x9E)),
    }
}

/// Ordena as entries (já filtradas) por GRUPO de status, pagina cada
/// grupo em janelas de `FINANCE_PAGE_SIZE` e devolve apenas as linhas da
/// página atual de cada grupo. Preenche o cabeçalho (label/contagem/total
/// do grupo INTEIRO) na 1ª linha e os metadados de paginação em todas.
/// `pages` é ajustado (clamp) para páginas válidas — o caller persiste.
pub(crate) fn group_and_paginate(mut rows: Vec<EntryRaw>, pages: &mut GroupPages) -> Vec<EntryRaw> {
    rows.sort_by_key(|r| group_rank(&r.status));
    let mut out: Vec<EntryRaw> = Vec::new();
    let mut i = 0;
    while i < rows.len() {
        let r0 = group_rank(&rows[i].status);
        let mut j = i;
        let mut total = 0.0_f64;
        while j < rows.len() && group_rank(&rows[j].status) == r0 {
            total += rows[j].amount.abs();
            j += 1;
        }
        let count = j - i;
        let key = group_key(&rows[i].status);
        let (label, color) = group_label(&rows[i].status);
        let total_pages = count.div_ceil(FINANCE_PAGE_SIZE).max(1) as i32;
        // Página válida (clamp) e persistência do valor corrigido.
        let page = pages.get(key).clamp(1, total_pages);
        pages.set(key, page);

        let start = i + (page as usize - 1) * FINANCE_PAGE_SIZE;
        let end = (start + FINANCE_PAGE_SIZE).min(j);
        let meta = format!(
            "{} {} · {}",
            count,
            if count == 1 { "conta" } else { "contas" },
            money_br(total)
        );
        let window_len = end - start;
        for (offset, src) in rows[start..end].iter().enumerate() {
            let mut row = src.clone();
            row.group_first = offset == 0;
            row.group_last = offset == window_len - 1;
            row.group_page = page;
            row.group_pages = total_pages;
            row.group_status = key.to_string();
            if offset == 0 {
                row.group_label = label.to_string();
                row.group_color = color;
                row.group_meta = meta.clone();
            }
            out.push(row);
        }
        i = j;
    }
    out
}

pub(crate) fn apply_snapshot(ui: &MainWindow, snap: Snapshot) {
    let kpis_model: ModelRc<FinanceKpi> = ModelRc::new(VecModel::from(snap.kpis));
    ui.set_finance_kpis(kpis_model);

    let active = ui.get_finance_active_tab().to_string();
    let active = if active.is_empty() { "receivable".to_string() } else { active };
    let tabs: Vec<FinanceTab> = snap
        .tabs
        .into_iter()
        .map(|mut t| {
            t.selected = t.key.as_str() == active.as_str();
            t
        })
        .collect();
    ui.set_finance_tabs(ModelRc::new(VecModel::from(tabs)));

    // Lista unificada: receber + pagar aparecem juntos. `active-tab`
    // segue como property residual (ainda escolhe o kind padrão de
    // alguns lugares), mas não filtra a lista.
    let search = ui.get_finance_search_query().to_lowercase();
    let status_filter = ui.get_finance_status_filter().to_string();
    let filtered_raw: Vec<EntryRaw> = snap
        .entries
        .into_iter()
        .filter(|e| match_search(&search, e))
        .filter(|e| match_status_filter(&status_filter, &e.status))
        .collect();
    // Paginação por grupo (10 por página). Páginas vêm das props do
    // MainWindow (autoridade no backend); clamp corrige valores fora de
    // faixa e é persistido de volta.
    let mut pages = GroupPages {
        overdue: ui.get_finance_page_overdue(),
        pending: ui.get_finance_page_pending(),
        paid: ui.get_finance_page_settled(),
        cancelled: ui.get_finance_page_cancelled(),
    };
    let windowed = group_and_paginate(filtered_raw, &mut pages);
    ui.set_finance_page_overdue(pages.overdue);
    ui.set_finance_page_pending(pages.pending);
    ui.set_finance_page_settled(pages.paid);
    ui.set_finance_page_cancelled(pages.cancelled);
    let filtered: Vec<FinanceEntryRow> = windowed.into_iter().map(to_slint_row).collect();
    ui.set_finance_entries(ModelRc::new(VecModel::from(filtered)));

    let cats: Vec<FinanceCategoryOption> = snap
        .categories
        .into_iter()
        .map(|c| FinanceCategoryOption {
            id: SharedString::from(c.id),
            name: SharedString::from(c.name),
            color: SharedString::from(c.color),
            scope: SharedString::from(c.scope),
        })
        .collect();
    ui.set_finance_categories(ModelRc::new(VecModel::from(cats)));

    let cash_flow: Vec<FinanceCashFlowPoint> = snap
        .cash_flow
        .into_iter()
        .map(|p| FinanceCashFlowPoint {
            label: SharedString::from(p.label),
            inflow_progress: p.inflow_progress,
            outflow_progress: p.outflow_progress,
            balance_progress: p.balance_progress,
            tooltip: SharedString::from(p.tooltip),
            today: p.today,
        })
        .collect();
    ui.set_finance_cash_flow(ModelRc::new(VecModel::from(cash_flow)));
    ui.set_finance_cash_flow_summary(SharedString::from(snap.cash_flow_summary));

    // ── Calendário ──
    ui.set_finance_cal_title(SharedString::from(snap.cal_title));
    ui.set_finance_cal_month_total(SharedString::from(snap.cal_month_total));
    let cells: Vec<FinanceCalendarCell> = snap
        .cal_cells
        .into_iter()
        .map(|c| FinanceCalendarCell {
            day: c.day,
            in_month: c.in_month,
            count: c.count,
            total_display: SharedString::from(c.total_display),
            total_tone: SharedString::from(c.total_tone),
            today: c.today,
            selected: c.selected,
        })
        .collect();
    ui.set_finance_cal_cells(ModelRc::new(VecModel::from(cells)));
    ui.set_finance_cal_selected(SharedString::from(snap.cal_selected_key));
    ui.set_finance_cal_detail_header(SharedString::from(snap.cal_detail_header));
    ui.set_finance_cal_detail_summary(SharedString::from(snap.cal_detail_summary));
    let detail_rows: Vec<FinanceDayRow> = snap
        .cal_detail_rows
        .into_iter()
        .map(|r| FinanceDayRow {
            id: SharedString::from(r.id),
            title: SharedString::from(r.title),
            subtitle: SharedString::from(r.subtitle),
            amount_display: SharedString::from(r.amount_display),
            amount_tone: SharedString::from(r.amount_tone),
            status_label: SharedString::from(r.status_label),
            status: SharedString::from(r.status),
            can_settle: r.can_settle,
            kind: SharedString::from(r.kind),
        })
        .collect();
    ui.set_finance_cal_detail_rows(ModelRc::new(VecModel::from(detail_rows)));
}

pub(crate) fn match_search(needle: &str, e: &EntryRaw) -> bool {
    if needle.is_empty() {
        return true;
    }
    e.description.to_lowercase().contains(needle)
        || e.party_name.to_lowercase().contains(needle)
        || e.category_name.to_lowercase().contains(needle)
}

/// Filtro de status enviado pelos chips da UI. Mapeia para os valores
/// brutos vindos de `EntryRaw::status` (já normalizados em
/// "pending"/"scheduled"/"overdue"/"paid"/"received"/"cancelled").
pub(crate) fn match_status_filter(filter: &str, status: &str) -> bool {
    match filter {
        "open" => matches!(status, "pending" | "scheduled"),
        "overdue" => status == "overdue",
        "settled" => matches!(status, "paid" | "received"),
        _ => true, // "all" (e default seguro)
    }
}

pub(crate) fn to_slint_row(r: EntryRaw) -> FinanceEntryRow {
    FinanceEntryRow {
        id: SharedString::from(r.id),
        kind: SharedString::from(r.kind),
        description: SharedString::from(r.description),
        party_name: SharedString::from(r.party_name),
        category_name: SharedString::from(r.category_name),
        category_color: r.category_color,
        amount_display: SharedString::from(r.amount_display),
        amount_color: r.amount_color,
        due_date_display: SharedString::from(r.due_date_display),
        days_display: SharedString::from(r.days_display),
        status: SharedString::from(r.status),
        status_label: SharedString::from(r.status_label),
        status_color: r.status_color,
        payment_method_label: SharedString::from(r.payment_method_label),
        action_label: SharedString::from(r.action_label),
        installment_label: SharedString::from(r.installment_label),
        group_first: r.group_first,
        group_label: SharedString::from(r.group_label),
        group_meta: SharedString::from(r.group_meta),
        group_color: r.group_color,
        group_last: r.group_last,
        group_page: r.group_page,
        group_pages: r.group_pages,
        group_status: SharedString::from(r.group_status),
    }
}

