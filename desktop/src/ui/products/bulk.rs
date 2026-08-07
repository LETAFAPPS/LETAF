//! Cadastro em massa de produtos: exportar/importar CSV.
//!
//! Regras aplicadas (AI_RULES.md §1/§7/§8/§11):
//! - Toda escrita passa pelo `ProductService` (validação + offline-first);
//!   a UI só dispara e mostra o resultado.
//! - Upsert por `id`: linha com id existente ATUALIZA (preservando foto,
//!   variações, descontos e demais campos fora do CSV); linha sem id (ou id
//!   desconhecido) CRIA um produto novo SEM foto.
//! - CSV separado por `;` (Excel pt-BR), UTF-8 com BOM; números com vírgula.

use std::collections::HashMap;
use std::sync::Arc;

use rust_decimal::Decimal;
use slint::ComponentHandle;
use tokio::sync::Notify;
use uuid::Uuid;

use letaf_core::product::model::{BalanceMode, Product};

use crate::context::DesktopState;
use crate::format::{parse_money_br, parse_money_br_f64};
use crate::MainWindow;

use super::super::helpers::show_toast;
use crate::ProductsState;

/// Colunas do CSV, na ordem de exportação. Os cabeçalhos são a chave de
/// leitura na importação (posição das colunas é flexível).
const HEADER: &[&str] = &[
    "id",
    "nome",
    "descricao",
    "categoria",
    "subcategoria",
    "preco",
    "preco_custo",
    "estoque",
    "estoque_minimo",
    "unidade",
    "codigo_barras",
    "estoque_ilimitado",
    "ativo",
    "visivel_web",
    "desconto_tipo",
    "desconto_valor",
    "desconto_qtd_min",
    "desconto_faixas",
    "disponibilidade",
    "adicionais",
    "variacoes",
];

/// Separador dos nomes de grupos de adicionais dentro de uma célula.
const ADDON_SEP: char = '|';

pub(crate) fn setup_bulk(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    setup_export(ui, state, handle);
    setup_import(ui, state, handle, sync_notify);
}

// ── Exportar ─────────────────────────────────────────────────────

fn setup_export(ui: &MainWindow, state: &DesktopState, handle: &tokio::runtime::Handle) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.global::<ProductsState>().on_export_products(move || {
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            let products = state.product_service.find_all(cid).await.unwrap_or_default();
            let cats = state.category_service.find_all(cid).await.unwrap_or_default();
            let subs = state.subcategory_service.find_all(cid).await.unwrap_or_default();
            let addons = state.addon_group_service.find_all(cid).await.unwrap_or_default();
            // id → nome (para exibir nomes legíveis no CSV).
            let cat_name: HashMap<Uuid, String> =
                cats.into_iter().map(|c| (c.base.id, c.name)).collect();
            let sub_name: HashMap<Uuid, String> =
                subs.into_iter().map(|s| (s.base.id, s.name)).collect();
            let addon_name: HashMap<Uuid, String> =
                addons.into_iter().map(|a| (a.base.id, a.name)).collect();
            let csv = build_csv(&products, &cat_name, &sub_name, &addon_name);

            let chosen = tokio::task::spawn_blocking(move || {
                rfd::FileDialog::new()
                    .set_file_name("produtos.csv")
                    .add_filter("CSV", &["csv"])
                    .save_file()
            })
            .await
            .ok()
            .flatten();
            let Some(path) = chosen else { return };
            let res = std::fs::write(&path, csv.as_bytes());
            let n = products.len();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    match res {
                        Ok(()) => show_toast(&ui, &format!("{n} produto(s) exportado(s)"), "success"),
                        Err(e) => {
                            tracing::warn!("export de produtos falhou: {e}");
                            show_toast(&ui, "Não foi possível salvar o arquivo", "error");
                        }
                    }
                }
            });
        });
    });
}

/// Monta o conteúdo CSV (com BOM) a partir dos produtos.
fn build_csv(
    products: &[Product],
    cat_name: &HashMap<Uuid, String>,
    sub_name: &HashMap<Uuid, String>,
    addon_name: &HashMap<Uuid, String>,
) -> String {
    let mut out = String::from("\u{FEFF}"); // BOM: Excel abre em UTF-8.
    out.push_str(&HEADER.join(";"));
    out.push_str("\r\n");
    for p in products {
        let name_of = |map: &HashMap<Uuid, String>, id: Option<Uuid>| {
            id.and_then(|i| map.get(&i)).cloned().unwrap_or_default()
        };
        // Adicionais: nomes dos grupos separados por "|".
        let adicionais = p
            .addon_group_ids
            .iter()
            .filter_map(|id| addon_name.get(id))
            .cloned()
            .collect::<Vec<_>>()
            .join(&ADDON_SEP.to_string());
        let row = [
            p.base.id.to_string(),
            p.name.clone(),
            p.description.clone().unwrap_or_default(),
            name_of(cat_name, p.category_id),
            name_of(sub_name, p.subcategory_id),
            fmt_dec(p.price),
            fmt_dec(p.cost_price),
            fmt_num(p.stock_quantity),
            fmt_num(p.min_stock),
            p.unit.clone(),
            p.barcode.clone().unwrap_or_default(),
            bool_pt(p.unlimited_stock),
            bool_pt(p.active),
            bool_pt(p.web_visible),
            p.discount_kind.clone().unwrap_or_default(),
            fmt_dec(p.discount_value),
            p.discount_min_qty.map(fmt_num).unwrap_or_default(),
            p.discount_tiers.clone().unwrap_or_default(),
            p.availability_schedule.clone().unwrap_or_default(),
            adicionais,
            p.variations.clone().unwrap_or_default(),
        ];
        let escaped: Vec<String> = row.iter().map(|f| csv_field(f)).collect();
        out.push_str(&escaped.join(";"));
        out.push_str("\r\n");
    }
    out
}

// ── Importar ─────────────────────────────────────────────────────

fn setup_import(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.global::<ProductsState>().on_import_products(move || {
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let notify = sync_notify.clone();
        handle.spawn(async move {
            let chosen = tokio::task::spawn_blocking(move || {
                rfd::FileDialog::new().add_filter("CSV", &["csv"]).pick_file()
            })
            .await
            .ok()
            .flatten();
            let Some(path) = chosen else { return };
            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("leitura do CSV falhou: {e}");
                    toast_import(&ui_weak, "Não foi possível ler o arquivo", "error");
                    return;
                }
            };
            let outcome = import_rows(&state, &content).await;
            if outcome.created + outcome.updated > 0 {
                notify.notify_one();
            }
            let msg = format!(
                "Importação: {} criado(s), {} atualizado(s), {} ignorado(s)",
                outcome.created, outcome.updated, outcome.skipped
            );
            let ui_weak2 = ui_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak2.upgrade() {
                    let kind = if outcome.created + outcome.updated > 0 { "success" } else { "info" };
                    show_toast(&ui, &msg, kind);
                    // Recarrega a lista do banco (estado canônico).
                    ui.global::<ProductsState>().invoke_refresh_products();
                }
            });
        });
    });
}

#[derive(Default)]
struct Outcome {
    created: u32,
    updated: u32,
    skipped: u32,
}

/// Processa o CSV linha a linha via `ProductService` (upsert por id).
async fn import_rows(state: &DesktopState, content: &str) -> Outcome {
    let mut outcome = Outcome::default();
    let records = parse_csv(content, ';');
    let mut iter = records.into_iter().filter(|r| r.iter().any(|c| !c.trim().is_empty()));
    // 1ª linha não vazia = cabeçalho → mapa nome-da-coluna → índice.
    let Some(header) = iter.next() else { return outcome };
    let cols = header_index(&header);

    let cid = state.company_id();
    let existentes = state.product_service.find_all(cid).await.unwrap_or_default();
    let by_id: HashMap<Uuid, Product> =
        existentes.into_iter().map(|p| (p.base.id, p)).collect();
    // nome (minúsculo) → id, para resolver as colunas por nome.
    let cats = state.category_service.find_all(cid).await.unwrap_or_default();
    let cat_by_name: HashMap<String, Uuid> =
        cats.into_iter().map(|c| (c.name.trim().to_lowercase(), c.base.id)).collect();
    let subs = state.subcategory_service.find_all(cid).await.unwrap_or_default();
    let sub_by_name: HashMap<String, Uuid> =
        subs.into_iter().map(|s| (s.name.trim().to_lowercase(), s.base.id)).collect();
    let addons = state.addon_group_service.find_all(cid).await.unwrap_or_default();
    let addon_by_name: HashMap<String, Uuid> =
        addons.into_iter().map(|a| (a.name.trim().to_lowercase(), a.base.id)).collect();
    let names = Names { cat: &cat_by_name, sub: &sub_by_name, addon: &addon_by_name };

    for cells in iter {
        let get = |key: &str| -> String {
            cols.get(key)
                .and_then(|&i| cells.get(i))
                .map(|s| s.trim().to_string())
                .unwrap_or_default()
        };
        // Coluna presente no cabeçalho? Distingue "célula vazia" (limpa) de
        // "coluna ausente" (mantém o valor atual na atualização).
        let has = |key: &str| -> bool { cols.contains_key(key) };
        let name = get("nome");
        if name.is_empty() {
            outcome.skipped += 1;
            continue;
        }
        let existing = get("id").parse::<Uuid>().ok().and_then(|id| by_id.get(&id));

        let ok = if let Some(p) = existing {
            apply_update(state, cid, p, &get, &has, &names).await
        } else {
            apply_create(state, cid, &get, &names).await
        };
        match (ok, existing.is_some()) {
            (true, true) => outcome.updated += 1,
            (true, false) => outcome.created += 1,
            (false, _) => outcome.skipped += 1,
        }
    }
    outcome
}

/// Mapas nome→id para resolver categoria, subcategoria e adicionais.
struct Names<'a> {
    cat: &'a HashMap<String, Uuid>,
    sub: &'a HashMap<String, Uuid>,
    addon: &'a HashMap<String, Uuid>,
}

/// Resolve um id a partir do nome (case-insensitive).
/// Vazio → `None`; encontrado → `Some`; não encontrado → `fallback`.
fn resolve_id_by_name(
    cell: &str,
    by_name: &HashMap<String, Uuid>,
    fallback: Option<Uuid>,
) -> Option<Uuid> {
    let key = cell.trim().to_lowercase();
    if key.is_empty() {
        None
    } else {
        by_name.get(&key).copied().or(fallback)
    }
}

/// Nomes de grupos de adicionais (separados por `|`) → ids existentes.
/// Nomes desconhecidos são ignorados.
fn resolve_addons(cell: &str, by_name: &HashMap<String, Uuid>) -> Vec<Uuid> {
    cell.split(ADDON_SEP)
        .map(|n| n.trim().to_lowercase())
        .filter(|n| !n.is_empty())
        .filter_map(|n| by_name.get(&n).copied())
        .collect()
}

/// Cria um produto novo (sem foto). `true` em sucesso.
async fn apply_create(
    state: &DesktopState,
    cid: Uuid,
    get: &impl Fn(&str) -> String,
    names: &Names<'_>,
) -> bool {
    let unit = { let u = get("unidade"); if u.is_empty() { "un".to_string() } else { u } };
    let res = state
        .product_service
        .create(
            cid,
            get("nome"),
            opt(get("descricao")),
            resolve_id_by_name(&get("categoria"), names.cat, None),
            resolve_id_by_name(&get("subcategoria"), names.sub, None),
            dec_or_none(&get("preco")),
            dec_or_none(&get("preco_custo")),
            num_or(&get("estoque"), 0.0),
            num_or(&get("estoque_minimo"), 0.0),
            parse_bool(&get("estoque_ilimitado")),
            opt(get("codigo_barras")),
            unit,
            BalanceMode::Weight,
            None, // sem foto — cadastrada depois manualmente
            None, // cover_color
            opt(get("disponibilidade")),
            opt(get("desconto_tipo")),
            dec_or_none(&get("desconto_valor")),
            num_or_none(&get("desconto_qtd_min")),
            opt(get("desconto_faixas")),
            resolve_addons(&get("adicionais"), names.addon),
            Vec::new(), // ficha técnica não vem do CSV nesta fase
            opt(get("variacoes")),
        )
        .await;
    log_err("criar", &get("nome"), res.err())
}

/// Atualiza um produto existente. Coluna AUSENTE do arquivo mantém o valor
/// atual; coluna presente com célula vazia limpa o campo (opcionais). Foto,
/// `balance_mode` e cor não estão no CSV e são sempre preservados.
async fn apply_update(
    state: &DesktopState,
    cid: Uuid,
    p: &Product,
    get: &impl Fn(&str) -> String,
    has: &impl Fn(&str) -> bool,
    names: &Names<'_>,
) -> bool {
    let unit = { let u = get("unidade"); if u.is_empty() { p.unit.clone() } else { u } };
    let category_id = if has("categoria") {
        resolve_id_by_name(&get("categoria"), names.cat, p.category_id)
    } else {
        p.category_id
    };
    let subcategory_id = if has("subcategoria") {
        resolve_id_by_name(&get("subcategoria"), names.sub, p.subcategory_id)
    } else {
        p.subcategory_id
    };
    let addon_group_ids = if has("adicionais") {
        resolve_addons(&get("adicionais"), names.addon)
    } else {
        p.addon_group_ids.clone()
    };
    let discount_min_qty = if has("desconto_qtd_min") {
        num_or_none(&get("desconto_qtd_min"))
    } else {
        p.discount_min_qty
    };
    let res = state
        .product_service
        .update(
            cid,
            p.base.id,
            get("nome"),
            cell_str(has, get, "descricao", p.description.clone()),
            category_id,
            subcategory_id,
            cell_dec(has, get, "preco", p.price),
            cell_dec(has, get, "preco_custo", p.cost_price),
            num_or(&get("estoque"), p.stock_quantity),
            num_or(&get("estoque_minimo"), p.min_stock),
            bool_or(&get("estoque_ilimitado"), p.unlimited_stock),
            cell_str(has, get, "codigo_barras", p.barcode.clone()),
            unit,
            p.balance_mode,
            p.image_data.clone(),
            p.cover_color.clone(),
            cell_str(has, get, "disponibilidade", p.availability_schedule.clone()),
            cell_str(has, get, "desconto_tipo", p.discount_kind.clone()),
            cell_dec(has, get, "desconto_valor", p.discount_value),
            discount_min_qty,
            cell_str(has, get, "desconto_faixas", p.discount_tiers.clone()),
            addon_group_ids,
            p.ingredients.clone(), // preserva a ficha técnica (não vem do CSV)
            cell_str(has, get, "variacoes", p.variations.clone()),
        )
        .await;
    log_err("atualizar", &get("nome"), res.err())
}

/// Campo texto opcional: coluna presente → valor da célula (vazio = limpa);
/// ausente → mantém o atual.
fn cell_str(
    has: &impl Fn(&str) -> bool,
    get: &impl Fn(&str) -> String,
    key: &str,
    existing: Option<String>,
) -> Option<String> {
    if has(key) { opt(get(key)) } else { existing }
}

/// Idem para campo `Decimal`.
fn cell_dec(
    has: &impl Fn(&str) -> bool,
    get: &impl Fn(&str) -> String,
    key: &str,
    existing: Option<Decimal>,
) -> Option<Decimal> {
    if has(key) { dec_or_none(&get(key)) } else { existing }
}

fn log_err(acao: &str, nome: &str, err: Option<letaf_core::error::CoreError>) -> bool {
    match err {
        None => true,
        Some(e) => {
            tracing::warn!("import: não deu para {acao} '{nome}': {e}");
            false
        }
    }
}

// ── Helpers de formatação/parse ──────────────────────────────────

/// Vazio → `None`; senão `Some(trim)`.
fn opt(s: String) -> Option<String> {
    let t = s.trim();
    if t.is_empty() { None } else { Some(t.to_string()) }
}

/// Célula numérica → `Decimal` (aceita vírgula/ponto); vazia → `None`.
fn dec_or_none(s: &str) -> Option<Decimal> {
    if s.trim().is_empty() { None } else { parse_money_br(s) }
}

/// Célula numérica → `f64`; vazia/ inválida → `default`.
fn num_or(s: &str, default: f64) -> f64 {
    if s.trim().is_empty() { default } else { parse_money_br_f64(s).unwrap_or(default) }
}

/// Célula numérica → `Option<f64>`; vazia → `None`.
fn num_or_none(s: &str) -> Option<f64> {
    if s.trim().is_empty() { None } else { parse_money_br_f64(s) }
}

fn bool_or(s: &str, default: bool) -> bool {
    if s.trim().is_empty() { default } else { parse_bool(s) }
}

fn parse_bool(s: &str) -> bool {
    matches!(s.trim().to_lowercase().as_str(), "sim" | "s" | "true" | "1" | "x" | "yes")
}

fn bool_pt(b: bool) -> String {
    if b { "sim".to_string() } else { "nao".to_string() }
}

/// Decimal → texto com vírgula decimal; `None` → vazio.
fn fmt_dec(v: Option<Decimal>) -> String {
    v.map(|d| d.normalize().to_string().replace('.', ",")).unwrap_or_default()
}

/// f64 → texto com vírgula decimal (sem casas supérfluas).
fn fmt_num(v: f64) -> String {
    let s = format!("{v}");
    s.replace('.', ",")
}

/// Escapa um campo CSV: aspas quando contém `;`, `"` ou quebra de linha.
fn csv_field(s: &str) -> String {
    if s.contains([';', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Cabeçalho → índice por nome normalizado (minúsculo, sem acento).
fn header_index(header: &[String]) -> HashMap<String, usize> {
    header
        .iter()
        .enumerate()
        .map(|(i, h)| (normalize_key(h), i))
        .collect()
}

/// Normaliza o nome da coluna: minúsculas, sem acento, trim.
fn normalize_key(s: &str) -> String {
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

fn toast_import(ui_weak: &slint::Weak<MainWindow>, msg: &str, kind: &str) {
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
/// de aspas.
fn parse_csv(content: &str, sep: char) -> Vec<Vec<String>> {
    let mut records = Vec::new();
    let mut record: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut dirty = false; // algo foi visto na linha atual
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
    // Remove BOM do primeiro campo do primeiro registro.
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
        // Campo entre aspas com ';' e aspas escapadas dentro.
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
