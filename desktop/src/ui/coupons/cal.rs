use std::sync::Arc;
use rust_decimal::prelude::ToPrimitive;

use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use letaf_core::coupon::model::Coupon;

use crate::{CalDay, CouponData, MainWindow};

use super::form::{discount_summary, int_str, num_str, type_label};

// ── Calendário pop-up dos campos de validade ──────────────────────
//
// Reutiliza um único `CalendarPopup` para os dois campos do modal de
// cupom (`coupon-valid-from` e `coupon-valid-until`). `target` controla
// qual campo recebe a data; cada campo guarda seu próprio mês visível
// para que abrir-fechar não perca o contexto do operador.

#[derive(Clone, Copy)]
pub(crate) struct MonthCursor {
    pub(crate) year: i32,
    pub(crate) month: u32,
    pub(crate) selected: Option<NaiveDate>,
}

impl MonthCursor {
    fn today() -> Self {
        let t = Local::now().date_naive();
        Self { year: t.year(), month: t.month(), selected: None }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct CouponCalState {
    pub(crate) from: MonthCursor,
    pub(crate) until: MonthCursor,
}

impl CouponCalState {
    fn new() -> Self {
        Self { from: MonthCursor::today(), until: MonthCursor::today() }
    }
}

pub(crate) type CouponCalHandle = Arc<std::sync::Mutex<CouponCalState>>;

pub(crate) fn setup_coupon_cal(ui: &MainWindow) {
    let cal: CouponCalHandle = Arc::new(std::sync::Mutex::new(CouponCalState::new()));

    // Abre o pop-up para um dos campos. Inicializa o mês visível com
    // a data já preenchida (se houver) ou hoje. Marca `coupon-cal-target`
    // para que o `if` no main.slint mostre o CalendarPopup.
    let ui_weak = ui.as_weak();
    let cal_open = cal.clone();
    ui.on_coupon_cal_open(move |target| {
        let Some(ui) = ui_weak.upgrade() else { return };
        let t = target.to_string();
        if t != "from" && t != "until" { return; }
        let current_value = if t == "from" {
            ui.get_coupon_valid_from().to_string()
        } else {
            ui.get_coupon_valid_until().to_string()
        };
        let parsed = NaiveDate::parse_from_str(&current_value, "%d/%m/%Y").ok();
        if let Ok(mut g) = cal_open.lock() {
            let cursor = if t == "from" { &mut g.from } else { &mut g.until };
            if let Some(d) = parsed {
                cursor.year = d.year();
                cursor.month = d.month();
                cursor.selected = Some(d);
            }
        }
        ui.set_coupon_cal_target(SharedString::from(t));
        apply_coupon_cal(&ui, &cal_open);
    });

    let ui_weak = ui.as_weak();
    ui.on_coupon_cal_close(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_coupon_cal_target(SharedString::from(""));
        }
    });

    let ui_weak = ui.as_weak();
    let cal_prev = cal.clone();
    ui.on_coupon_cal_prev(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        let target = ui.get_coupon_cal_target().to_string();
        if let Ok(mut g) = cal_prev.lock() {
            let cursor = match target.as_str() {
                "from" => &mut g.from,
                "until" => &mut g.until,
                _ => return,
            };
            let (y, m) = (cursor.year, cursor.month);
            let (ny, nm) = if m == 1 { (y - 1, 12) } else { (y, m - 1) };
            cursor.year = ny;
            cursor.month = nm;
        }
        apply_coupon_cal(&ui, &cal_prev);
    });

    let ui_weak = ui.as_weak();
    let cal_next = cal.clone();
    ui.on_coupon_cal_next(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        let target = ui.get_coupon_cal_target().to_string();
        if let Ok(mut g) = cal_next.lock() {
            let cursor = match target.as_str() {
                "from" => &mut g.from,
                "until" => &mut g.until,
                _ => return,
            };
            let (y, m) = (cursor.year, cursor.month);
            let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
            cursor.year = ny;
            cursor.month = nm;
        }
        apply_coupon_cal(&ui, &cal_next);
    });

    let ui_weak = ui.as_weak();
    let cal_pick = cal;
    ui.on_coupon_cal_pick(move |ymd| {
        let Some(ui) = ui_weak.upgrade() else { return };
        let ymd_s = ymd.to_string();
        let target = ui.get_coupon_cal_target().to_string();
        if target != "from" && target != "until" { return; }

        if ymd_s.is_empty() {
            if target == "from" {
                ui.set_coupon_valid_from(SharedString::from(""));
            } else {
                ui.set_coupon_valid_until(SharedString::from(""));
            }
            if let Ok(mut g) = cal_pick.lock() {
                let cursor = if target == "from" { &mut g.from } else { &mut g.until };
                cursor.selected = None;
            }
        } else if let Ok(d) = NaiveDate::parse_from_str(&ymd_s, "%Y-%m-%d") {
            let br = d.format("%d/%m/%Y").to_string();
            if target == "from" {
                ui.set_coupon_valid_from(SharedString::from(br));
            } else {
                ui.set_coupon_valid_until(SharedString::from(br));
            }
            ui.set_coupon_error_validity(SharedString::default());
            if let Ok(mut g) = cal_pick.lock() {
                let cursor = if target == "from" { &mut g.from } else { &mut g.until };
                cursor.year = d.year();
                cursor.month = d.month();
                cursor.selected = Some(d);
            }
        }
        ui.set_coupon_cal_target(SharedString::from(""));
    });
}

pub(crate) fn apply_coupon_cal(ui: &MainWindow, cal: &CouponCalHandle) {
    let target = ui.get_coupon_cal_target().to_string();
    if target != "from" && target != "until" { return; }
    let snap = cal.lock().ok().map(|g| if target == "from" { g.from } else { g.until })
        .unwrap_or_else(MonthCursor::today);
    let today = Local::now().date_naive();
    ui.set_coupon_cal_title(SharedString::from(format!("{} · {}", month_pt(snap.month), snap.year)));
    ui.set_coupon_cal_days(ModelRc::new(VecModel::from(
        build_cal_days(snap.year, snap.month, snap.selected, today),
    )));
}

pub(crate) fn build_cal_days(
    year: i32,
    month: u32,
    selected: Option<NaiveDate>,
    today: NaiveDate,
) -> Vec<CalDay> {
    let first = NaiveDate::from_ymd_opt(year, month, 1)
        .unwrap_or_else(|| Local::now().date_naive());
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

pub(crate) fn month_pt(m: u32) -> &'static str {
    match m {
        1 => "Janeiro", 2 => "Fevereiro", 3 => "Março", 4 => "Abril",
        5 => "Maio", 6 => "Junho", 7 => "Julho", 8 => "Agosto",
        9 => "Setembro", 10 => "Outubro", 11 => "Novembro", 12 => "Dezembro",
        _ => "",
    }
}

/// Converte `Coupon` (domínio) → `CouponData` (Slint). Labels/resumo
/// pré-formatados em pt-BR (UI não faz lógica — §1/§3).
pub(crate) fn to_coupon_data(c: &Coupon) -> CouponData {
    let date_str = |d: Option<NaiveDateTime>| -> SharedString {
        d.map(|x| SharedString::from(x.date().format("%d/%m/%Y").to_string()))
            .unwrap_or_default()
    };
    CouponData {
        id: SharedString::from(c.base.id.to_string()),
        title: SharedString::from(c.title.as_str()),
        code: SharedString::from(c.code.as_str()),
        coupon_type: SharedString::from(c.coupon_type.as_str()),
        coupon_type_label: SharedString::from(type_label(&c.coupon_type)),
        discount_kind: SharedString::from(c.discount_kind.as_str()),
        discount_summary: SharedString::from(discount_summary(c)),
        discount_value: num_str(c.discount_value.to_f64().unwrap_or(0.0)),
        min_order_value: num_str(c.min_order_value.to_f64().unwrap_or(0.0)),
        max_discount: num_str(c.max_discount.to_f64().unwrap_or(0.0)),
        per_user_limit: int_str(c.per_user_limit),
        usage_limit: int_str(c.usage_limit),
        valid_from: date_str(c.valid_from),
        valid_until: date_str(c.valid_until),
        active: c.active,
    }
}
