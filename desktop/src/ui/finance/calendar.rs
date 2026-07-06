
use chrono::{Datelike, Duration, Local, NaiveDate};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use uuid::Uuid;


use crate::context::DesktopState;
use crate::{
    CalDay, MainWindow,
};

use super::setup::reapply;
use super::state::{CalStateHandle, CustomersHandle, DueCalState, DueCalStateHandle};
use super::snapshot::month_pt;
use super::modal::build_party_rows;

// ── Picker de cliente/fornecedor ────────────────────────────────

pub(crate) fn setup_party_picker(ui: &MainWindow, customers: CustomersHandle) {
    let ui_weak = ui.as_weak();
    ui.on_finance_open_party_picker(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_finance_party_search(SharedString::from(""));
            ui.set_finance_show_party_picker(true);
        }
    });
    let ui_weak = ui.as_weak();
    ui.on_finance_close_party_picker(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_finance_show_party_picker(false);
        }
    });
    let ui_weak = ui.as_weak();
    let customers_filter = customers.clone();
    ui.on_finance_party_search_changed(move |q| {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_finance_party_search(SharedString::from(q.to_string()));
            let all = customers_filter
                .lock()
                .ok()
                .map(|g| g.clone())
                .unwrap_or_default();
            let rows = build_party_rows(&all, q.as_str());
            ui.set_finance_party_options(ModelRc::new(VecModel::from(rows)));
        }
    });
    let ui_weak = ui.as_weak();
    let customers_pick = customers;
    ui.on_finance_pick_party(move |id| {
        if let Some(ui) = ui_weak.upgrade() {
            let id_s = id.to_string();
            if id_s.is_empty() {
                ui.set_finance_form_party(SharedString::from(""));
                ui.set_finance_form_party_id(SharedString::from(""));
            } else if let Ok(uuid) = Uuid::parse_str(&id_s) {
                if let Ok(all) = customers_pick.lock() {
                    if let Some(c) = all.iter().find(|c| c.base.id == uuid) {
                        ui.set_finance_form_party(SharedString::from(c.name.clone()));
                        ui.set_finance_form_party_id(SharedString::from(id_s));
                    }
                }
            }
            ui.set_finance_show_party_picker(false);
        }
    });
}

// ── Calendário pop-up do vencimento ─────────────────────────────

pub(crate) fn setup_due_cal(ui: &MainWindow, cal: DueCalStateHandle) {
    // Estado inicial visível assim que o modal abre.
    apply_due_cal_to_ui(ui, &cal);

    let ui_weak = ui.as_weak();
    let cal_toggle = cal.clone();
    ui.on_finance_due_cal_toggle(move || {
        if let Some(ui) = ui_weak.upgrade() {
            let now_open = ui.get_finance_due_cal_open();
            ui.set_finance_due_cal_open(!now_open);
            if !now_open {
                apply_due_cal_to_ui(&ui, &cal_toggle);
            }
        }
    });

    let ui_weak = ui.as_weak();
    let cal_prev = cal.clone();
    ui.on_finance_due_cal_prev(move || {
        if let Ok(mut g) = cal_prev.lock() {
            let (y, m) = (g.year, g.month);
            let (ny, nm) = if m == 1 { (y - 1, 12) } else { (y, m - 1) };
            g.year = ny;
            g.month = nm;
        }
        if let Some(ui) = ui_weak.upgrade() {
            apply_due_cal_to_ui(&ui, &cal_prev);
        }
    });

    let ui_weak = ui.as_weak();
    let cal_next = cal.clone();
    ui.on_finance_due_cal_next(move || {
        if let Ok(mut g) = cal_next.lock() {
            let (y, m) = (g.year, g.month);
            let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
            g.year = ny;
            g.month = nm;
        }
        if let Some(ui) = ui_weak.upgrade() {
            apply_due_cal_to_ui(&ui, &cal_next);
        }
    });

    let ui_weak = ui.as_weak();
    let cal_pick = cal;
    ui.on_finance_due_cal_pick(move |ymd| {
        let Some(ui) = ui_weak.upgrade() else { return };
        let ymd_s = ymd.to_string();
        if ymd_s.is_empty() {
            // Limpar — volta o vencimento para vazio.
            ui.set_finance_form_due_date(SharedString::from(""));
            if let Ok(mut g) = cal_pick.lock() {
                g.selected = None;
            }
        } else if let Ok(d) = NaiveDate::parse_from_str(&ymd_s, "%Y-%m-%d") {
            ui.set_finance_form_due_date(SharedString::from(d.format("%d/%m/%Y").to_string()));
            if let Ok(mut g) = cal_pick.lock() {
                g.year = d.year();
                g.month = d.month();
                g.selected = Some(d);
            }
        }
        ui.set_finance_due_cal_open(false);
        apply_due_cal_to_ui(&ui, &cal_pick);
    });
}

pub(crate) fn apply_due_cal_to_ui(ui: &MainWindow, cal: &DueCalStateHandle) {
    let snap = cal.lock().ok().map(|g| *g).unwrap_or_else(DueCalState::today);
    let today = Local::now().date_naive();
    let title = format!("{} · {}", month_pt(snap.month), snap.year);
    let days = build_due_cal_days(snap.year, snap.month, snap.selected, today);
    ui.set_finance_due_cal_title(SharedString::from(title));
    ui.set_finance_due_cal_days(ModelRc::new(VecModel::from(days)));
}

/// Gera 42 células (6 semanas × 7 dias, domingo a sábado) para o
/// `CalendarPopup`. Dias fora do mês ficam com `label = ""` e
/// `ymd = ""` (não-clicáveis).
pub(crate) fn build_due_cal_days(
    year: i32,
    month: u32,
    selected: Option<NaiveDate>,
    today: NaiveDate,
) -> Vec<CalDay> {
    let first = NaiveDate::from_ymd_opt(year, month, 1)
        .unwrap_or_else(|| Local::now().date_naive());
    // Domingo da semana que contém o dia 1.
    let offset = first.weekday().num_days_from_sunday() as i64;
    let grid_start = first - Duration::days(offset);
    (0..42)
        .map(|i| {
            let d = grid_start + Duration::days(i as i64);
            let in_month = d.month() == month && d.year() == year;
            CalDay {
                label: SharedString::from(if in_month { d.day().to_string() } else { String::new() }),
                ymd: SharedString::from(if in_month { d.format("%Y-%m-%d").to_string() } else { String::new() }),
                selected: in_month && Some(d) == selected,
                in_range: false,
                today: in_month && d == today,
            }
        })
        .collect()
}

// ── Setters do form (recurrence/installments/category) ──────────

pub(crate) fn setup_set_recurrence(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_finance_set_recurrence(move |k| {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_finance_form_recurrence(SharedString::from(k.to_string()));
            // Recorrência ≠ once força installments para 1 (regra do service).
            if k.as_str() != "once" {
                ui.set_finance_form_installments(1);
            }
        }
    });
}

pub(crate) fn setup_set_installments(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_finance_set_installments(move |n| {
        if let Some(ui) = ui_weak.upgrade() {
            let bounded = n.clamp(1, 60);
            ui.set_finance_form_installments(bounded);
            // Parcelamento ≠ 1 força recurrence = once.
            if bounded > 1 {
                ui.set_finance_form_recurrence(SharedString::from("once"));
            }
        }
    });
}

pub(crate) fn setup_set_category(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_finance_set_category(move |id| {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_finance_form_category_id(SharedString::from(id.to_string()));
        }
    });
}

// ── Calendário: toggle modo e navegação ──────────────────────────

pub(crate) fn setup_set_view(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    cal: CalStateHandle,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_finance_set_view(move |mode| {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_finance_view_mode(SharedString::from(mode.to_string()));
        }
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let cal = cal.clone();
        handle.spawn(async move {
            reapply(&ui_weak, &state, &cal).await;
        });
    });
}

pub(crate) fn setup_cal_prev(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    cal: CalStateHandle,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_finance_cal_prev(move || {
        if let Ok(mut g) = cal.lock() {
            let (y, m) = (g.year, g.month);
            let (ny, nm) = if m == 1 { (y - 1, 12) } else { (y, m - 1) };
            g.year = ny;
            g.month = nm;
            g.selected_day = None; // troca de mês limpa seleção
        }
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let cal = cal.clone();
        handle.spawn(async move {
            reapply(&ui_weak, &state, &cal).await;
        });
    });
}

pub(crate) fn setup_cal_next(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    cal: CalStateHandle,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_finance_cal_next(move || {
        if let Ok(mut g) = cal.lock() {
            let (y, m) = (g.year, g.month);
            let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
            g.year = ny;
            g.month = nm;
            g.selected_day = None;
        }
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let cal = cal.clone();
        handle.spawn(async move {
            reapply(&ui_weak, &state, &cal).await;
        });
    });
}

pub(crate) fn setup_cal_today(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    cal: CalStateHandle,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_finance_cal_today(move || {
        if let Ok(mut g) = cal.lock() {
            let t = Local::now().date_naive();
            g.year = t.year();
            g.month = t.month();
            g.selected_day = Some(t.day());
        }
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let cal = cal.clone();
        handle.spawn(async move {
            reapply(&ui_weak, &state, &cal).await;
        });
    });
}

pub(crate) fn setup_cal_select_day(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    cal: CalStateHandle,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_finance_cal_select_day(move |day| {
        if day < 1 {
            return;
        }
        if let Ok(mut g) = cal.lock() {
            // Click no mesmo dia já selecionado = limpa.
            g.selected_day = if g.selected_day == Some(day as u32) {
                None
            } else {
                Some(day as u32)
            };
        }
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let cal = cal.clone();
        handle.spawn(async move {
            reapply(&ui_weak, &state, &cal).await;
        });
    });
}

