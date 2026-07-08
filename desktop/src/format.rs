//! Utilitários de formatação de exibição para campos de cliente.
//!
//! Regras aplicadas (AI_RULES.md §1, §2, §8):
//! - Módulo de responsabilidade única: apenas formatação de display
//! - Funções puras sem efeitos colaterais
//! - Sem acesso a banco ou dependências externas

use chrono::{Local, NaiveDateTime, TimeZone};

/// Converte um `NaiveDateTime` (gravado em UTC no banco) para o fuso horário
/// local do dispositivo. Usado apenas na camada de apresentação — o banco
/// continua armazenando UTC (boa prática multi-tenant).
fn to_local(utc: NaiveDateTime) -> chrono::DateTime<Local> {
    Local.from_utc_datetime(&utc)
}

/// Formata a data de um pedido em `DD/MM/AAAA` no fuso local.
pub fn format_order_date(utc: NaiveDateTime) -> String {
    to_local(utc).format("%d/%m/%Y").to_string()
}

/// Formata o horário de um pedido em `HH:MM` no fuso local.
pub fn format_order_time(utc: NaiveDateTime) -> String {
    to_local(utc).format("%H:%M").to_string()
}

/// Formata um valor monetário em pt-BR COM sinal explícito:
/// - 0 → `R$ 0,00`
/// - positivo → `+R$ 12,34`
/// - negativo → `-R$ 5,00`
///
/// Útil em colunas de movimentação/diferença onde o sinal é informação.
pub fn money_br_signed(v: rust_decimal::Decimal) -> String {
    use rust_decimal::Decimal;
    if v.abs() < Decimal::new(5, 3) {
        money_br(Decimal::ZERO)
    } else if v > Decimal::ZERO {
        format!("+{}", money_br(v))
    } else {
        money_br(v)
    }
}

/// Formata um valor monetário em pt-BR: `R$ 2.530,00` (com `−` para
/// negativos). Centralizado aqui para que listagens, cards e relatórios
/// usem a mesma máscara — qualquer divergência ficaria visível ao
/// operador (AI_RULES.md §1, §8).
pub fn money_br(v: rust_decimal::Decimal) -> String {
    let cents = letaf_core::money::to_cents(v);
    let neg = cents < 0;
    let cents = cents.abs();
    let reais = cents / 100;
    let dec = cents % 100;
    let s = reais.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push('.');
        }
        out.push(ch);
    }
    let int_part: String = out.chars().rev().collect();
    format!("R$ {}{},{:02}", if neg { "-" } else { "" }, int_part, dec)
}

/// Formata em pt-BR um valor vindo do banco como `f64` (converte para
/// `Decimal` via `from_db_f64` e reusa `money_br`). Fonte ÚNICA do par
/// "f64 do storage → máscara BRL", eliminando o wrapper reescrito em cada
/// tela (AI_RULES.md §8/§14).
pub fn money_br_f64(v: f64) -> String {
    money_br(letaf_core::money::from_db_f64(v))
}

/// Formata uma quantidade de estoque conforme a unidade.
///
/// Regras aplicadas (AI_RULES.md §1, §8):
/// - `un` / `cx`: número inteiro (sem decimais)
/// - `kg`: 3 casas decimais (precisão de grama)
/// - Outras unidades: trata como inteiro por padrão
pub fn format_stock(qty: f64, unit: &str) -> String {
    match unit {
        "kg" => format!("{qty:.3}"),
        _    => format!("{}", qty.round() as i64),
    }
}

// O status de estoque agora é fonte única em `letaf_core::product::model::Product::stock_status`
// (usa o `min_stock` configurado pelo operador). A heurística antiga
// "≤ 5 unidades / ≤ 1 kg" foi removida — o core decide com base no
// mínimo configurado, e a apresentação só formata o rótulo
// (`desktop/src/ui/products.rs::stock_status_label`).

/// Aplica máscara de telefone progressiva a partir de dígitos brutos ou formatados.
///
/// - 10 dígitos → `(XX) XXXX-XXXX` (fixo)
/// - 11 dígitos → `(XX) XXXXX-XXXX` (celular)
pub fn format_phone(raw: &str) -> String {
    let d: String = raw.chars().filter(|c| c.is_ascii_digit()).take(11).collect();
    let len = d.len();
    match len {
        0 => String::new(),
        1..=2 => format!("({}", &d),
        3..=6 => format!("({}) {}", &d[..2], &d[2..]),
        7..=10 => format!("({}) {}-{}", &d[..2], &d[2..6], &d[6..]),
        11 => format!("({}) {}-{}", &d[..2], &d[2..7], &d[7..]),
        _ => format!("({}) {}-{}", &d[..2], &d[2..7], &d[7..11]),
    }
}

/// Aplica máscara de documento progressiva a partir de dígitos brutos ou formatados.
///
/// - 11 dígitos → CPF `000.000.000-00`
/// - 14 dígitos → CNPJ `00.000.000/0000-00`
pub fn format_document(raw: &str) -> String {
    let d: String = raw.chars().filter(|c| c.is_ascii_digit()).take(14).collect();
    let len = d.len();
    match len {
        0 => String::new(),
        1..=3 => d,
        4..=6 => format!("{}.{}", &d[..3], &d[3..]),
        7..=9 => format!("{}.{}.{}", &d[..3], &d[3..6], &d[6..]),
        10..=11 => format!("{}.{}.{}-{}", &d[..3], &d[3..6], &d[6..9], &d[9..]),
        12 => format!("{}.{}.{}/{}", &d[..2], &d[2..5], &d[5..8], &d[8..12]),
        13 => format!("{}.{}.{}/{}-{}", &d[..2], &d[2..5], &d[5..8], &d[8..12], &d[12..13]),
        _ => format!("{}.{}.{}/{}-{}", &d[..2], &d[2..5], &d[5..8], &d[8..12], &d[12..14]),
    }
}

/// Aplica máscara de data `DD/MM/AAAA` a partir de dígitos brutos ou
/// já formatados. Strings parciais são exibidas progressivamente
/// (mesma lógica de `format_phone`/`format_zip_code`).
pub fn format_date_br(raw: &str) -> String {
    let d: String = raw.chars().filter(|c| c.is_ascii_digit()).take(8).collect();
    let len = d.len();
    match len {
        0 => String::new(),
        1..=2 => d,
        3..=4 => format!("{}/{}", &d[..2], &d[2..]),
        _ => format!("{}/{}/{}", &d[..2], &d[2..4], &d[4..]),
    }
}

/// Máscara do número do cartão: dígitos em grupos de 4
/// (`0000 0000 0000 0000`). Aceita até 19 dígitos (bandeiras longas).
pub fn format_card_number(raw: &str) -> String {
    let d: String = raw.chars().filter(|c| c.is_ascii_digit()).take(19).collect();
    d.as_bytes()
        .chunks(4)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Máscara de validade `MM/AA` (só dígitos, máximo 4).
pub fn format_card_expiry(raw: &str) -> String {
    let d: String = raw.chars().filter(|c| c.is_ascii_digit()).take(4).collect();
    if d.len() <= 2 {
        d
    } else {
        format!("{}/{}", &d[..2], &d[2..])
    }
}

/// CVV: apenas dígitos, máximo 4 (sem separador).
pub fn format_cvv(raw: &str) -> String {
    raw.chars().filter(|c| c.is_ascii_digit()).take(4).collect()
}

/// Aplica máscara de CEP a partir de dígitos brutos ou formatados.
/// 8 dígitos → `00000-000`. Strings parciais são exibidas progressivamente.
pub fn format_zip_code(raw: &str) -> String {
    let d: String = raw.chars().filter(|c| c.is_ascii_digit()).take(8).collect();
    let len = d.len();
    match len {
        0 => String::new(),
        1..=5 => d,
        _ => format!("{}-{}", &d[..5], &d[5..]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phone_mobile_complete() {
        assert_eq!(format_phone("53434343430"), "(53) 43434-3430");
    }

    #[test]
    fn phone_landline_complete() {
        assert_eq!(format_phone("5343434343"), "(53) 4343-4343");
    }

    #[test]
    fn phone_already_formatted() {
        assert_eq!(format_phone("(53) 43434-3430"), "(53) 43434-3430");
    }

    #[test]
    fn zip_complete() {
        assert_eq!(format_zip_code("86026150"), "86026-150");
    }

    #[test]
    fn zip_already_formatted() {
        assert_eq!(format_zip_code("86026-150"), "86026-150");
    }

    #[test]
    fn zip_partial_under_5_returns_raw_digits() {
        assert_eq!(format_zip_code("8602"), "8602");
    }

    #[test]
    fn document_cpf_complete() {
        assert_eq!(format_document("12345678901"), "123.456.789-01");
    }

    #[test]
    fn document_cnpj_complete() {
        assert_eq!(format_document("12345678000195"), "12.345.678/0001-95");
    }

    #[test]
    fn document_already_formatted() {
        assert_eq!(format_document("123.456.789-01"), "123.456.789-01");
    }

    #[test]
    fn card_number_groups_of_four() {
        assert_eq!(format_card_number("4111111111111111"), "4111 1111 1111 1111");
        // Aceita texto já parcialmente mascarado e ignora não-dígitos.
        assert_eq!(format_card_number("4111 1111 1"), "4111 1111 1");
    }

    #[test]
    fn card_expiry_mm_aa() {
        assert_eq!(format_card_expiry("0828"), "08/28");
        assert_eq!(format_card_expiry("0"), "0");
        assert_eq!(format_card_expiry("12/2"), "12/2");
    }

    #[test]
    fn cvv_digits_only() {
        assert_eq!(format_cvv("12a3"), "123");
        assert_eq!(format_cvv("12345"), "1234");
    }
}
