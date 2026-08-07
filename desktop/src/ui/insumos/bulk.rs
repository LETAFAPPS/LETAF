//! Cadastro em massa de INSUMOS: exportar/importar CSV (espelha produtos).
//! Upsert por `id`: linha com id existente ATUALIZA; sem id (ou id
//! desconhecido) CRIA. Toda escrita passa pelo `InsumoService` (validação +
//! offline-first). Reaproveita o motor de CSV em `crate::ui::csv` (§14).

use std::collections::HashMap;
use std::sync::Arc;

use slint::ComponentHandle;
use tokio::sync::Notify;
use uuid::Uuid;

use letaf_core::insumo::model::Insumo;

use crate::context::DesktopState;
use crate::ui::csv::{
    bool_or, bool_pt, cell_dec, cell_str, csv_field, dec_or_none, fmt_dec, fmt_num, header_index,
    log_err, num_or, opt, parse_csv, toast_import,
};
use crate::MainWindow;

use super::super::helpers::show_toast;
use crate::InsumosState;

const HEADER: &[&str] = &[
    "id",
    "nome",
    "descricao",
    "unidade",
    "estoque",
    "estoque_minimo",
    "custo",
    "codigo_barras",
    "ativo",
];

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
    ui.global::<InsumosState>().on_export_insumos(move || {
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            let insumos = state.insumo_service.find_all(cid).await.unwrap_or_default();
            let csv = build_csv(&insumos);
            let chosen = tokio::task::spawn_blocking(move || {
                rfd::FileDialog::new()
                    .set_file_name("insumos.csv")
                    .add_filter("CSV", &["csv"])
                    .save_file()
            })
            .await
            .ok()
            .flatten();
            let Some(path) = chosen else { return };
            let res = std::fs::write(&path, csv.as_bytes());
            let n = insumos.len();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    match res {
                        Ok(()) => show_toast(&ui, &format!("{n} insumo(s) exportado(s)"), "success"),
                        Err(e) => {
                            tracing::warn!("export de insumos falhou: {e}");
                            show_toast(&ui, "Não foi possível salvar o arquivo", "error");
                        }
                    }
                }
            });
        });
    });
}

fn build_csv(insumos: &[Insumo]) -> String {
    let mut out = String::from("\u{FEFF}");
    out.push_str(&HEADER.join(";"));
    out.push_str("\r\n");
    for i in insumos {
        let row = [
            i.base.id.to_string(),
            i.name.clone(),
            i.description.clone().unwrap_or_default(),
            i.unit.clone(),
            fmt_num(i.stock_quantity),
            fmt_num(i.min_stock),
            fmt_dec(i.cost_price),
            i.barcode.clone().unwrap_or_default(),
            bool_pt(i.active),
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
    ui.global::<InsumosState>().on_import_insumos(move || {
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
                    tracing::warn!("leitura do CSV de insumos falhou: {e}");
                    toast_import(&ui_weak, "Não foi possível ler o arquivo", "error");
                    return;
                }
            };
            let out = import_rows(&state, &content).await;
            if out.created + out.updated > 0 {
                notify.notify_one();
            }
            let msg = format!(
                "Importação: {} criado(s), {} atualizado(s), {} ignorado(s)",
                out.created, out.updated, out.skipped
            );
            let ui_weak2 = ui_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak2.upgrade() {
                    let kind = if out.created + out.updated > 0 { "success" } else { "info" };
                    show_toast(&ui, &msg, kind);
                    ui.global::<InsumosState>().invoke_refresh_insumos();
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

async fn import_rows(state: &DesktopState, content: &str) -> Outcome {
    let mut outcome = Outcome::default();
    let records = parse_csv(content, ';');
    let mut iter = records.into_iter().filter(|r| r.iter().any(|c| !c.trim().is_empty()));
    let Some(header) = iter.next() else { return outcome };
    let cols = header_index(&header);

    let cid = state.company_id();
    let existentes = state.insumo_service.find_all(cid).await.unwrap_or_default();
    let by_id: HashMap<Uuid, Insumo> = existentes.into_iter().map(|i| (i.base.id, i)).collect();

    for cells in iter {
        let get = |key: &str| -> String {
            cols.get(key).and_then(|&idx| cells.get(idx)).map(|s| s.trim().to_string()).unwrap_or_default()
        };
        let has = |key: &str| -> bool { cols.contains_key(key) };
        let name = get("nome");
        if name.is_empty() {
            outcome.skipped += 1;
            continue;
        }
        let existing = get("id").parse::<Uuid>().ok().and_then(|id| by_id.get(&id));

        let ok = if let Some(prev) = existing {
            let unit = { let u = get("unidade"); if u.is_empty() { prev.unit.clone() } else { u } };
            let res = state.insumo_service.update(
                cid,
                prev.base.id,
                name.clone(),
                cell_str(&has, &get, "descricao", prev.description.clone()),
                unit,
                num_or(&get("estoque"), prev.stock_quantity),
                num_or(&get("estoque_minimo"), prev.min_stock),
                cell_dec(&has, &get, "custo", prev.cost_price),
                cell_str(&has, &get, "codigo_barras", prev.barcode.clone()),
                bool_or(&get("ativo"), prev.active),
            ).await;
            log_err("atualizar", &name, res.err())
        } else {
            let unit = { let u = get("unidade"); if u.is_empty() { "un".to_string() } else { u } };
            let res = state.insumo_service.create(
                cid,
                name.clone(),
                opt(get("descricao")),
                unit,
                num_or(&get("estoque"), 0.0),
                num_or(&get("estoque_minimo"), 0.0),
                dec_or_none(&get("custo")),
                opt(get("codigo_barras")),
                if has("ativo") { bool_or(&get("ativo"), true) } else { true },
            ).await;
            log_err("criar", &name, res.err())
        };
        match (ok, existing.is_some()) {
            (true, true) => outcome.updated += 1,
            (true, false) => outcome.created += 1,
            (false, _) => outcome.skipped += 1,
        }
    }
    outcome
}
