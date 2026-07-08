
use chrono::NaiveDate;
use slint::{Color, SharedString};


fn money_br(v: f64) -> String {
    crate::format::money_br_f64(v)
}
use crate::{
    FinanceKpi, FinanceTab,
};


// ── Helpers ──────────────────────────────────────────────────────

pub(crate) fn kpi(label: &str, value: &str, sub: &str, color: Color, tone: &str) -> FinanceKpi {
    kpi_ex(label, value, sub, color, tone, false)
}

/// Como `kpi`, mas com `value_neutral` (valor em text-primary, segue o tema).
pub(crate) fn kpi_ex(label: &str, value: &str, sub: &str, color: Color, tone: &str, value_neutral: bool) -> FinanceKpi {
    FinanceKpi {
        label: SharedString::from(label),
        value_display: SharedString::from(value),
        sub_text: SharedString::from(sub),
        accent_color: color,
        sub_tone: SharedString::from(tone),
        value_neutral,
    }
}

pub(crate) fn tab(key: &str, label: &str, count: i64, total: f64) -> FinanceTab {
    FinanceTab {
        key: SharedString::from(key),
        label: SharedString::from(label),
        count_display: SharedString::from(count.to_string()),
        total_display: SharedString::from(money_br(total)),
        selected: false,
    }
}

/// Valor com sinal: positivo → "R$ 1.200,00", negativo → "R$ -1.200,00".
pub(crate) fn money_signed(v: f64) -> String {
    if v >= 0.0 {
        money_br(v)
    } else {
        format!("R$ -{}", money_br(-v).trim_start_matches("R$ "))
    }
}

pub(crate) fn balance_tone(v: f64) -> &'static str {
    if v < 0.0 {
        "neg"
    } else if v > 0.0 {
        "pos"
    } else {
        "neutral"
    }
}

pub(crate) fn days_label(today: NaiveDate, due: NaiveDate, settled: bool) -> String {
    if settled {
        return format!("pago em {}", due.format("%d/%m"));
    }
    let diff = (due - today).num_days();
    match diff {
        0 => "hoje".into(),
        1 => "amanhã".into(),
        d if d > 1 => format!("em {} dias", d),
        d => format!("vencido há {} dias", -d),
    }
}

pub(crate) fn parse_amount(s: &str) -> Option<f64> {
    let cleaned = s
        .trim()
        .replace("R$", "")
        .replace([' ', '.'], "")
        .replace(',', ".");
    cleaned.parse::<f64>().ok()
}

pub(crate) fn parse_date_br(s: &str) -> Option<NaiveDate> {
    let s = s.trim();
    NaiveDate::parse_from_str(s, "%d/%m/%Y")
        .ok()
        .or_else(|| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
}

pub(crate) fn parse_hex_color(hex: &str) -> Color {
    let h = hex.trim_start_matches('#');
    if h.len() != 6 {
        return Color::from_rgb_u8(0xBD, 0xBD, 0xBD);
    }
    let r = u8::from_str_radix(&h[0..2], 16).unwrap_or(0xBD);
    let g = u8::from_str_radix(&h[2..4], 16).unwrap_or(0xBD);
    let b = u8::from_str_radix(&h[4..6], 16).unwrap_or(0xBD);
    Color::from_rgb_u8(r, g, b)
}

pub(crate) fn payment_method_label(s: &str) -> String {
    match s {
        "cash" => "Dinheiro".into(),
        "pix" => "PIX".into(),
        "credit" => "Cartão crédito".into(),
        "debit" => "Cartão débito".into(),
        other if !other.is_empty() => other.to_string(),
        _ => String::new(),
    }
}
