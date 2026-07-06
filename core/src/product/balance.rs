//! Reconhecimento de códigos de barras emitidos por balanças.
//!
//! Regras aplicadas (AI_RULES.md §1, §8, §11):
//! - Lógica de domínio pura — sem dependências de UI ou banco.
//! - Validação rigorosa: prefixo, comprimento, somente dígitos, check digit.
//!
//! ## Padrão suportado
//!
//! EAN-13 com prefixo `2` (uso interno do estabelecimento, padrão brasileiro):
//!
//! ```text
//! 2  NNNNNN  XXXXX  C
//! │  │       │      └─ dígito verificador EAN-13
//! │  │       └──────── 5 dígitos: peso (gramas) OU preço (centavos)
//! │  └──────────────── 6 dígitos: código do produto (campo `barcode`)
//! └─────────────────── prefixo "2"
//! ```
//!
//! O parser **não interpreta** os 5 dígitos centrais — devolve o número bruto
//! em `raw_value`. Cabe ao caller (PDV) consultar `Product.balance_mode` e
//! decidir:
//!
//! - `BalanceMode::Weight` → `raw_value` é peso em gramas
//!   (`total = price_per_kg * raw_value / 1000.0`)
//! - `BalanceMode::Price`  → `raw_value` é preço em centavos
//!   (`total = raw_value / 100.0`)
//!
//! Para mapear ao cadastro: o produto kg deve ter `barcode` = os 6 dígitos
//! intermediários (ex.: `"200001"`).

/// Resultado do parse de um código de barras de balança.
///
/// `product_code` casa com `Product.barcode` no banco;
/// `raw_value` é o número da janela variável (semântica definida pelo modo).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BalanceCode {
    pub product_code: String,
    pub raw_value: u32,
}

/// Decodifica um EAN-13 emitido por balança (prefixo `2`).
///
/// Retorna `Some(BalanceCode)` quando:
/// - O código tem exatamente 13 caracteres
/// - Todos os caracteres são dígitos
/// - O primeiro dígito é `2`
/// - O check digit EAN-13 está correto
///
/// Caso contrário, retorna `None` (caller decide o fallback — geralmente
/// trata como código de produto unitário comum).
pub fn parse_balance_barcode(scanned: &str) -> Option<BalanceCode> {
    if scanned.len() != 13 {
        return None;
    }
    if !scanned.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if !scanned.starts_with('2') {
        return None;
    }
    if !is_valid_ean13_check(scanned) {
        return None;
    }
    let product_code = scanned[1..7].to_string();
    let raw_value: u32 = scanned[7..12].parse().ok()?;
    Some(BalanceCode { product_code, raw_value })
}

/// Valida o dígito verificador EAN-13 (algoritmo padrão GS1).
///
/// Regras aplicadas (AI_RULES.md §11):
/// - Soma ponderada dos 12 primeiros dígitos (peso 1, 3, 1, 3, ...).
/// - Check digit = (10 - soma mod 10) mod 10.
fn is_valid_ean13_check(code: &str) -> bool {
    let digits: Vec<u32> = code.chars().filter_map(|c| c.to_digit(10)).collect();
    if digits.len() != 13 {
        return false;
    }
    let sum: u32 = digits[..12]
        .iter()
        .enumerate()
        .map(|(i, d)| if i % 2 == 0 { *d } else { *d * 3 })
        .sum();
    let expected = (10 - (sum % 10)) % 10;
    expected == digits[12]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Gera um EAN-13 válido a partir dos 12 primeiros dígitos.
    fn build_ean13(prefix12: &str) -> String {
        assert_eq!(prefix12.len(), 12);
        let digits: Vec<u32> = prefix12.chars().map(|c| c.to_digit(10).unwrap()).collect();
        let sum: u32 = digits
            .iter()
            .enumerate()
            .map(|(i, d)| if i % 2 == 0 { *d } else { *d * 3 })
            .sum();
        let check = (10 - (sum % 10)) % 10;
        format!("{prefix12}{check}")
    }

    #[test]
    fn parses_500g_picanha() {
        // produto 200001, 500g (interpretado como peso pelo PDV)
        let code = build_ean13("220000100500");
        let parsed = parse_balance_barcode(&code).expect("valid");
        assert_eq!(parsed.product_code, "200001");
        assert_eq!(parsed.raw_value, 500);
    }

    #[test]
    fn parses_max_value() {
        // produto 999999, raw_value 99999 (limite da janela de 5 dígitos)
        let code = build_ean13("299999999999");
        let parsed = parse_balance_barcode(&code).expect("valid");
        assert_eq!(parsed.product_code, "999999");
        assert_eq!(parsed.raw_value, 99999);
    }

    #[test]
    fn rejects_wrong_prefix() {
        // EAN-13 válido mas começando com 7 (varejo padrão) — não é balança
        let code = build_ean13("789012345678");
        assert_eq!(parse_balance_barcode(&code), None);
    }

    #[test]
    fn rejects_invalid_check_digit() {
        // Prefixo 2 mas com check digit errado (manualmente quebrado).
        let mut code = build_ean13("220000100500");
        let last = code.pop().unwrap();
        let bad = if last == '0' { '1' } else { '0' };
        code.push(bad);
        assert_eq!(parse_balance_barcode(&code), None);
    }

    #[test]
    fn rejects_wrong_length() {
        assert_eq!(parse_balance_barcode("2200001"), None);
        assert_eq!(parse_balance_barcode("22000010050099"), None);
    }

    #[test]
    fn rejects_non_digits() {
        assert_eq!(parse_balance_barcode("2200001ABCDE0"), None);
    }

    #[test]
    fn rejects_empty() {
        assert_eq!(parse_balance_barcode(""), None);
    }
}
