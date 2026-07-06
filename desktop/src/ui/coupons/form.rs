
use chrono::{NaiveDate, NaiveDateTime};
use rust_decimal::prelude::ToPrimitive;
use slint::SharedString;

use letaf_core::coupon::model::Coupon;

use crate::MainWindow;

use super::super::helpers::show_toast;

// ── Helpers ───────────────────────────────────────────────────────

pub(crate) struct CouponForm {
    pub(crate) title: String,
    pub(crate) code: String,
    pub(crate) coupon_type: String,
    pub(crate) discount_kind: String,
    pub(crate) discount_value: f64,
    pub(crate) min_order_value: f64,
    pub(crate) max_discount: f64,
    pub(crate) per_user_limit: i32,
    pub(crate) usage_limit: i32,
    pub(crate) valid_from: Option<NaiveDateTime>,
    pub(crate) valid_until: Option<NaiveDateTime>,
}

pub(crate) fn report_error(ui_weak: slint::Weak<MainWindow>, e: letaf_core::error::CoreError) {
    let msg = format!("Erro: {e}");
    let _ = slint::invoke_from_event_loop(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        show_toast(&ui, &msg, "error");
        ui.set_status_message(SharedString::from(msg));
    });
}

/// Lê e converte o form. Aterra os erros básicos (formato) inline e
/// devolve `None` — a validação de regra fica no service do core.
pub(crate) fn read_and_validate(ui: &MainWindow) -> Option<CouponForm> {
    ui.set_coupon_error_title(SharedString::default());
    ui.set_coupon_error_code(SharedString::default());
    ui.set_coupon_error_discount(SharedString::default());
    ui.set_coupon_error_min(SharedString::default());
    ui.set_coupon_error_per_user(SharedString::default());
    ui.set_coupon_error_usage(SharedString::default());
    ui.set_coupon_error_validity(SharedString::default());

    let title = ui.get_coupon_title().to_string();
    let code = ui.get_coupon_code().to_string();
    let coupon_type = ui.get_coupon_type().to_string();
    let discount_kind = ui.get_coupon_discount_kind().to_string();

    let mut ok = true;
    if title.trim().is_empty() {
        ui.set_coupon_error_title(SharedString::from("Informe o título"));
        ok = false;
    }
    if code.trim().is_empty() {
        ui.set_coupon_error_code(SharedString::from("Informe o código"));
        ok = false;
    }

    // Campo numérico obrigatório: vazio → "obrigatório"; preenchido
    // com algo não-numérico → "valor inválido". Aceita "0".
    let req_money = |raw: &str| -> Result<f64, &'static str> {
        let t = raw.trim();
        if t.is_empty() { return Err("Campo obrigatório"); }
        parse_money(t).map_err(|_| "Informe um valor numérico válido")
    };
    let req_int = |raw: &str| -> Result<i32, &'static str> {
        let t = raw.trim();
        if t.is_empty() { return Err("Campo obrigatório"); }
        parse_int(t).map_err(|_| "Informe um número inteiro válido")
    };

    // Frete grátis ignora valores de desconto (campos ocultos).
    let (discount_value, max_discount) = if coupon_type == "free_shipping" {
        (0.0, 0.0)
    } else {
        let dv = match req_money(&ui.get_coupon_discount_value()) {
            Ok(v) if v > 0.0 => v,
            Ok(_) => {
                ui.set_coupon_error_discount(SharedString::from("Valor deve ser maior que zero"));
                ok = false; 0.0
            }
            Err(m) => {
                ui.set_coupon_error_discount(SharedString::from(m));
                ok = false; 0.0
            }
        };
        let md = match req_money(&ui.get_coupon_max_discount()) {
            Ok(v) => v,
            Err(m) => {
                ui.set_coupon_error_discount(SharedString::from(format!("Desconto máximo: {m}")));
                ok = false; 0.0
            }
        };
        (dv, md)
    };

    let min_order_value = match req_money(&ui.get_coupon_min_order()) {
        Ok(v) => v,
        Err(m) => { ui.set_coupon_error_min(SharedString::from(m)); ok = false; 0.0 }
    };
    let per_user_limit = match req_int(&ui.get_coupon_per_user_limit()) {
        Ok(v) => v,
        Err(m) => { ui.set_coupon_error_per_user(SharedString::from(m)); ok = false; 0 }
    };
    let usage_limit = match req_int(&ui.get_coupon_usage_limit()) {
        Ok(v) => v,
        Err(m) => { ui.set_coupon_error_usage(SharedString::from(m)); ok = false; 0 }
    };

    let valid_from = match parse_date(&ui.get_coupon_valid_from(), false) {
        Ok(v) => v,
        Err(m) => { ui.set_coupon_error_validity(SharedString::from(m)); ok = false; None }
    };
    let valid_until = match parse_date(&ui.get_coupon_valid_until(), true) {
        Ok(v) => v,
        Err(m) => { ui.set_coupon_error_validity(SharedString::from(m)); ok = false; None }
    };

    if !ok { return None; }

    Some(CouponForm {
        title, code, coupon_type, discount_kind, discount_value,
        min_order_value, max_discount, per_user_limit, usage_limit,
        valid_from, valid_until,
    })
}

pub(crate) fn clear_form(ui: &MainWindow) {
    ui.set_editing_id(SharedString::default());
    ui.set_coupon_title(SharedString::default());
    ui.set_coupon_code(SharedString::default());
    ui.set_coupon_type(SharedString::from("standard"));
    ui.set_coupon_discount_kind(SharedString::from("fixed"));
    ui.set_coupon_discount_value(SharedString::default());
    ui.set_coupon_min_order(SharedString::default());
    ui.set_coupon_max_discount(SharedString::default());
    ui.set_coupon_per_user_limit(SharedString::default());
    ui.set_coupon_usage_limit(SharedString::default());
    ui.set_coupon_valid_from(SharedString::default());
    ui.set_coupon_valid_until(SharedString::default());
    ui.set_coupon_error_title(SharedString::default());
    ui.set_coupon_error_code(SharedString::default());
    ui.set_coupon_error_discount(SharedString::default());
    ui.set_coupon_error_min(SharedString::default());
    ui.set_coupon_error_per_user(SharedString::default());
    ui.set_coupon_error_usage(SharedString::default());
    ui.set_coupon_error_validity(SharedString::default());
}

/// "1.234,56" / "1234.56" / "" → f64. Vazio = 0.
pub(crate) fn parse_money(s: &str) -> Result<f64, ()> {
    let t = s.trim();
    if t.is_empty() { return Ok(0.0); }
    t.replace(['R', '$', ' '], "").replace(',', ".").parse::<f64>().map_err(|_| ())
}

pub(crate) fn parse_int(s: &str) -> Result<i32, ()> {
    let t = s.trim();
    if t.is_empty() { return Ok(0); }
    t.parse::<i32>().map_err(|_| ())
}

/// "DD/MM/AAAA" → Option<NaiveDateTime>. `end_of_day` = true usa
/// 23:59:59 (fim da validade); false usa 00:00:00. Vazio = sem limite.
pub(crate) fn parse_date(s: &str, end_of_day: bool) -> Result<Option<NaiveDateTime>, &'static str> {
    let t = s.trim();
    if t.is_empty() { return Ok(None); }
    let d = NaiveDate::parse_from_str(t, "%d/%m/%Y")
        .map_err(|_| "Data inválida (use DD/MM/AAAA)")?;
    let dt = if end_of_day {
        d.and_hms_opt(23, 59, 59)
    } else {
        d.and_hms_opt(0, 0, 0)
    };
    Ok(dt)
}

/// Número → string para pré-preencher o form de edição. Campos são
/// obrigatórios, então 0 vira "0" (não "") para sobreviver à
/// revalidação ao salvar.
pub(crate) fn num_str(v: f64) -> SharedString {
    if v.fract() == 0.0 {
        SharedString::from(format!("{}", v as i64))
    } else {
        SharedString::from(format!("{v:.2}"))
    }
}

pub(crate) fn int_str(v: i32) -> SharedString {
    SharedString::from(v.to_string())
}

pub(crate) fn type_label(t: &str) -> &'static str {
    match t {
        "first_purchase" => "Primeira Compra",
        "free_shipping" => "Frete Grátis",
        _ => "Padrão",
    }
}

pub(crate) fn discount_summary(c: &Coupon) -> String {
    if c.coupon_type == "free_shipping" {
        "Frete grátis".to_string()
    } else if c.discount_kind == "percent" {
        let dv = c.discount_value.to_f64().unwrap_or(0.0);
        if dv.fract() == 0.0 {
            format!("{}%", dv as i64)
        } else {
            format!("{dv}%")
        }
    } else {
        format!("R$ {:.2}", c.discount_value.to_f64().unwrap_or(0.0))
    }
}

