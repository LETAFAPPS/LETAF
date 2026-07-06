use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::MainWindow;
use crate::PrinterData;
use crate::context::DesktopState;

use super::super::helpers::show_toast;

/// "Imprimir página de teste" — gera texto monospace fixo e envia
/// para o `system_name` digitado no form (mesmo sem ter salvado).
/// Útil pra validar o cadastro: se sair do papel, o nome está
/// correto e a impressora alcançável.
pub(crate) fn setup_test_print(ui: &MainWindow, state: &DesktopState, handle: &tokio::runtime::Handle) {
    let ui_weak = ui.as_weak();
    let _state = state.clone();
    let handle = handle.clone();
    ui.on_printer_test_print(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        let system_name = ui_ref.get_printer_form_system_name().to_string();
        let paper_width = ui_ref.get_printer_form_paper_width();
        if system_name.trim().is_empty() {
            ui_ref.set_printer_form_error(SharedString::from(
                "Preencha 'Nome no sistema operacional' antes de testar."
            ));
            return;
        }
        let text = render_test_page(paper_width);
        let ui_weak = ui_weak.clone();
        handle.spawn_blocking(move || {
            // `send_to_default_printer` é síncrono (process::Command).
            // Usamos `spawn_blocking` para não estourar o tempo de uma
            // task tokio "normal" caso o spooler demore.
            let result = super::super::orders::send_to_default_printer(
                &text, "teste", Some(&system_name),
            );
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                match result {
                    Ok(()) => show_toast(&ui, "Página de teste enviada", "success"),
                    Err(e) => {
                        tracing::error!("teste de impressão falhou: {e}");
                        show_toast(&ui, "Falha ao imprimir teste (verifique nome no sistema)", "error");
                    }
                }
            });
        });
    });
}

/// Texto monospace da página de teste — confirma comunicação com a
/// impressora, mostra largura escolhida e data/hora.
pub(crate) fn render_test_page(paper_width: i32) -> String {
    let cols = if paper_width == 58 { 32 } else { 42 };
    let bar = "=".repeat(cols);
    let now = chrono::Utc::now()
        .with_timezone(&chrono::Local)
        .format("%d/%m/%Y %H:%M:%S")
        .to_string();
    let mut out = String::new();
    out.push_str(&bar); out.push('\n');
    out.push_str(&center_line("LETAF · TESTE DE IMPRESSORA", cols)); out.push('\n');
    out.push_str(&bar); out.push('\n');
    out.push_str(&format!("Data e Hora : {now}\n"));
    out.push_str(&format!("Largura     : {paper_width} mm ({cols} colunas)\n"));
    out.push_str(&bar); out.push('\n');
    out.push_str("Se voce esta lendo isso, a impressora\n");
    out.push_str("esta configurada corretamente.\n");
    out.push_str(&bar); out.push('\n');
    out.push('\n'); out.push('\n'); out.push('\n');
    out
}

pub(crate) fn center_line(s: &str, width: usize) -> String {
    let len = s.chars().count();
    if len >= width { return s.to_string(); }
    let pad = (width - len) / 2;
    format!("{}{}", " ".repeat(pad), s)
}

/// Enumera as impressoras instaladas no sistema operacional usando
/// utilitários nativos. Roda em `spawn_blocking` (chama `Command`).
///
/// Como o SO oferece descoberta nativa, evitamos uma crate dedicada
/// (que traria dependências FFI). Falha graciosamente: se o comando
/// não existir/não rodar, devolve `Vec::new()` e o operador vê
/// uma mensagem "Nenhuma impressora detectada".
pub(crate) fn setup_refresh_available_printers(ui: &MainWindow, handle: &tokio::runtime::Handle) {
    let ui_weak = ui.as_weak();
    let handle = handle.clone();
    ui.on_refresh_available_printers(move || {
        let ui_weak = ui_weak.clone();
        handle.spawn_blocking(move || {
            let list = enumerate_system_printers();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    let shared: Vec<SharedString> =
                        list.into_iter().map(SharedString::from).collect();
                    ui.set_printer_available_list(ModelRc::new(VecModel::from(shared)));
                }
            });
        });
    });
}

/// Lista impressoras conhecidas pelo sistema operacional.
///
/// - **Linux/macOS**: `lpstat -p` (CUPS). Saída no formato
///   `printer NOME is idle.  enabled since ...`.
/// - **Windows**: PowerShell `Get-Printer | Select-Object -ExpandProperty Name`
///   — uma linha por impressora.
///
/// Falhas (comando inexistente, CUPS não instalado, status != 0)
/// devolvem `Vec::new()` em vez de erro — UI mostra estado "vazio"
/// e operador pode revalidar manualmente.
pub(crate) fn enumerate_system_printers() -> Vec<String> {
    if cfg!(target_os = "windows") {
        enumerate_windows()
    } else {
        enumerate_unix()
    }
}

pub(crate) fn enumerate_unix() -> Vec<String> {
    let output = std::process::Command::new("lpstat").arg("-p").output();
    let Ok(out) = output else { return Vec::new(); };
    if !out.status.success() { return Vec::new(); }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut names = Vec::new();
    for line in stdout.lines() {
        // Linhas relevantes começam com "printer NAME ..." (en) ou
        // "impressora NAME ..." em locais pt-BR. Tratamos os dois.
        let trimmed = line.trim_start();
        let prefix = if trimmed.starts_with("printer ") {
            Some("printer ")
        } else if trimmed.starts_with("impressora ") {
            Some("impressora ")
        } else {
            None
        };
        if let Some(p) = prefix {
            let rest = &trimmed[p.len()..];
            if let Some(name) = rest.split_whitespace().next() {
                names.push(name.to_string());
            }
        }
    }
    names
}

pub(crate) fn enumerate_windows() -> Vec<String> {
    let output = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Get-Printer | Select-Object -ExpandProperty Name",
        ])
        .output();
    let Ok(out) = output else { return Vec::new(); };
    if !out.status.success() { return Vec::new(); }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

/// Mapeia `core::Printer` → `PrinterData` da UI, com labels em pt-BR.
pub(crate) fn to_printer_data(p: letaf_core::printer::model::Printer) -> PrinterData {
    let kind_label = match p.kind.as_str() {
        "order" => "Comanda",
        "kitchen" => "Cozinha",
        "fiscal" => "Nota Fiscal",
        _ => "—",
    };
    let paper_label = format!("{} mm", p.paper_width);
    // Resumo das categorias para exibir na listagem. Vazio = recebe
    // tudo (catch-all); senão "N categoria(s)" — exibimos a contagem
    // para não estourar o card com nomes longos. Os nomes ficam só
    // no modal de edição.
    let categories_label = if p.category_ids.is_empty() {
        "Todas as categorias".to_string()
    } else if p.category_ids.len() == 1 {
        "1 categoria".to_string()
    } else {
        format!("{} categorias", p.category_ids.len())
    };
    let category_ids: Vec<SharedString> = p.category_ids
        .iter()
        .map(|u| SharedString::from(u.to_string()))
        .collect();
    PrinterData {
        id: SharedString::from(p.base.id.to_string()),
        name: SharedString::from(p.name),
        kind: SharedString::from(p.kind),
        kind_label: SharedString::from(kind_label),
        system_name: SharedString::from(p.system_name),
        is_default: p.is_default,
        paper_width: p.paper_width,
        paper_label: SharedString::from(paper_label),
        category_ids: ModelRc::new(VecModel::from(category_ids)),
        categories_label: SharedString::from(categories_label),
    }
}
