
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use letaf_core::order::model::OrderStatus;

use chrono::{Datelike, Local, NaiveDate};

use crate::{CalDay, MainWindow};

use super::list::format_elapsed_since;

// ── Calendário de período (filtro de Pedidos) ─────────────────────────

/// "AAAA-MM-DD" → NaiveDate (vazio/ inválido = None).
pub(crate) fn parse_ymd(s: &str) -> Option<NaiveDate> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

fn month_pt(m: i32) -> &'static str {
    match m {
        1 => "Janeiro",
        2 => "Fevereiro",
        3 => "Março",
        4 => "Abril",
        5 => "Maio",
        6 => "Junho",
        7 => "Julho",
        8 => "Agosto",
        9 => "Setembro",
        10 => "Outubro",
        11 => "Novembro",
        12 => "Dezembro",
        _ => "",
    }
}

/// Texto do botão do filtro a partir do intervalo selecionado.
fn range_label(start: Option<NaiveDate>, end: Option<NaiveDate>) -> String {
    match (start, end) {
        (None, _) => "Período".to_string(),
        (Some(a), None) => a.format("%d/%m/%Y").to_string(),
        (Some(a), Some(b)) if a == b => a.format("%d/%m/%Y").to_string(),
        (Some(a), Some(b)) => {
            format!("{} – {}", a.format("%d/%m/%Y"), b.format("%d/%m/%Y"))
        }
    }
}

/// Reconstrói a grade do calendário (42 células = 6 semanas, domingo
/// como 1ª coluna) + título + rótulo do botão, a partir do mês
/// visível (`cal-year`/`cal-month`) e do intervalo selecionado.
fn cal_rebuild(ui: &MainWindow) {
    let today = Local::now().date_naive();
    let mut y = ui.get_cal_year();
    let mut m = ui.get_cal_month();

    let start = parse_ymd(ui.get_order_date_start().as_ref());
    let end = parse_ymd(ui.get_order_date_end().as_ref());

    if y == 0 || !(1..=12).contains(&m) {
        // Inicializa no mês do início selecionado, senão no mês atual.
        let base = start.unwrap_or(today);
        y = base.year();
        m = base.month() as i32;
        ui.set_cal_year(y);
        ui.set_cal_month(m);
    }

    let first = NaiveDate::from_ymd_opt(y, m as u32, 1).unwrap_or(today);
    let offset = first.weekday().num_days_from_sunday() as i64;
    let grid0 = first - chrono::Duration::days(offset);

    let mut days: Vec<CalDay> = Vec::with_capacity(42);
    for i in 0..42 {
        let d = grid0 + chrono::Duration::days(i);
        let in_month = d.month() as i32 == m && d.year() == y;
        let sel = Some(d) == start || Some(d) == end;
        let in_range = match (start, end) {
            (Some(a), Some(b)) => d >= a && d <= b,
            _ => sel,
        };
        days.push(CalDay {
            label: SharedString::from(if in_month {
                d.day().to_string()
            } else {
                String::new()
            }),
            ymd: SharedString::from(if in_month {
                d.format("%Y-%m-%d").to_string()
            } else {
                String::new()
            }),
            selected: in_month && sel,
            in_range: in_month && in_range,
            today: in_month && d == today,
        });
    }

    ui.set_cal_days(ModelRc::new(VecModel::from(days)));
    ui.set_cal_title(SharedString::from(format!("{} {}", month_pt(m), y)));
    ui.set_order_date_label(SharedString::from(range_label(start, end)));
}

/// Registra os callbacks do calendário de período. Lógica pura de
/// UI/estado (sem rede); ao escolher/limpar dispara o refresh para
/// Kanban e grade recalcularem com o intervalo (§1/§3).
pub(crate) fn setup_calendar(ui: &MainWindow) {
    // Abrir/fechar
    {
        let w = ui.as_weak();
        ui.on_cal_toggle(move || {
            let Some(ui) = w.upgrade() else { return };
            let open = !ui.get_cal_open();
            ui.set_cal_open(open);
            if open {
                cal_rebuild(&ui);
            }
        });
    }
    // Mês anterior
    {
        let w = ui.as_weak();
        ui.on_cal_prev(move || {
            let Some(ui) = w.upgrade() else { return };
            let mut y = ui.get_cal_year();
            let mut m = ui.get_cal_month() - 1;
            if m < 1 {
                m = 12;
                y -= 1;
            }
            ui.set_cal_year(y);
            ui.set_cal_month(m);
            cal_rebuild(&ui);
        });
    }
    // Próximo mês
    {
        let w = ui.as_weak();
        ui.on_cal_next(move || {
            let Some(ui) = w.upgrade() else { return };
            let mut y = ui.get_cal_year();
            let mut m = ui.get_cal_month() + 1;
            if m > 12 {
                m = 1;
                y += 1;
            }
            ui.set_cal_year(y);
            ui.set_cal_month(m);
            cal_rebuild(&ui);
        });
    }
    // Escolher um dia (define início ou fim do intervalo)
    {
        let w = ui.as_weak();
        ui.on_cal_pick(move |ymd| {
            let Some(ui) = w.upgrade() else { return };
            let Some(picked) = parse_ymd(ymd.as_str()) else { return };
            let start = parse_ymd(ui.get_order_date_start().as_ref());
            let end = parse_ymd(ui.get_order_date_end().as_ref());

            if start.is_none() || end.is_some() {
                // Sem início, ou intervalo já completo → recomeça.
                ui.set_order_date_start(ymd.clone());
                ui.set_order_date_end(SharedString::default());
            } else if let Some(s) = start {
                // Início definido, sem fim.
                if picked < s {
                    // Clicou antes do início → vira o novo início.
                    ui.set_order_date_start(ymd.clone());
                    ui.set_order_date_end(SharedString::default());
                } else {
                    ui.set_order_date_end(ymd.clone());
                }
            }
            cal_rebuild(&ui);
            ui.invoke_refresh_orders();
        });
    }
    // Limpar intervalo
    {
        let w = ui.as_weak();
        ui.on_cal_clear(move || {
            let Some(ui) = w.upgrade() else { return };
            ui.set_order_date_start(SharedString::default());
            ui.set_order_date_end(SharedString::default());
            cal_rebuild(&ui);
            ui.invoke_refresh_orders();
        });
    }
}

/// Callback: recalcula `detail-order.elapsed-display` a partir de
/// `created-at-iso`. Disparado pelo `Timer 60s` no painel direito.
///
/// Regras aplicadas (AI_RULES.md §1, §14):
/// - Lógica em Rust; o Slint só dispara o Timer.
/// - Mantém todos os outros campos do `detail-order` intactos
///   (mutação parcial de struct em Slint exige copiar tudo).
pub(crate) fn setup_refresh_order_elapsed(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_refresh_order_elapsed(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        let mut current = ui.get_detail_order();
        let iso = current.created_at_iso.to_string();
        if iso.is_empty() { return; }
        let created_at = match chrono::NaiveDateTime::parse_from_str(&iso, "%Y-%m-%dT%H:%M:%S") {
            Ok(dt) => dt,
            Err(_) => return,
        };
        let status = OrderStatus::from_str(current.status.as_str())
            .unwrap_or(OrderStatus::Pending);
        let new_elapsed = format_elapsed_since(created_at, &status);
        if new_elapsed != current.elapsed_display.as_str() {
            current.elapsed_display = SharedString::from(new_elapsed);
            ui.set_detail_order(current);
        }
    });
}

