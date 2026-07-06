
use slint::ComponentHandle;
use uuid::Uuid;

use letaf_core::order::model::OrderItem;

use crate::context::DesktopState;

use crate::MainWindow;

use super::super::helpers::show_toast;

/// Callback: gera a comanda como TEXTO MONOSPACE e envia direto para
/// a impressora padrão do sistema (sem browser).
///
/// Regras aplicadas (AI_RULES.md §1, §8, §11):
/// - Texto montado em Rust a partir do `Order` (não da UI), garantindo
///   coerência com o banco.
/// - Multi-plataforma: `lp` no Linux/macOS (CUPS), `Out-Printer` via
///   PowerShell no Windows. Compatível com impressora térmica (80 mm)
///   ou laser/inkjet de mesa.
/// - Se não houver impressora padrão configurada, o comando falha e
///   o operador vê um toast — sem perder o pedido.
pub(crate) fn setup_print_receipt_now(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_print_receipt_now(move |id_str, mode| {
        let id = match Uuid::parse_str(id_str.as_str()) {
            Ok(v) => v, Err(_) => return,
        };
        let mode_owned = mode.to_string();
        let state2 = state.clone();
        let ui_weak2 = ui_weak.clone();
        handle.spawn(async move {
            let cid = state2.company_id();
            let order = match state2.order_service.find_by_id(cid, id).await {
                Ok(Some(o)) => o,
                _ => {
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak2.upgrade() {
                            show_toast(&ui, "Pedido não encontrado", "error");
                        }
                    });
                    return;
                }
            };
            // Buscamos o cliente uma única vez aqui para os dois modos
            // (kitchen agora também mostra nome + telefone). Telefone
            // formatado pelo mesmo helper de Clientes/Configurações.
            let customer = state2.customer_service
                .find_by_id(cid, order.customer_id).await
                .ok().flatten();
            let customer_name = customer.as_ref()
                .map(|c| c.name.clone())
                .unwrap_or_else(|| "Cliente Presencial".into());
            let customer_phone = customer.as_ref()
                .and_then(|c| c.phone.clone())
                .filter(|s| !s.is_empty())
                .map(|p| crate::format::format_phone(&p))
                .unwrap_or_default();
            let suffix = if mode_owned == "kitchen" { "cozinha" } else { "comanda" };
            // ── COMANDA DO CLIENTE (kind=order) — caminho único ──
            // A comanda do cliente NÃO é dividida por categoria. Vai
            // tudo num só papel para a impressora padrão de "order".
            if mode_owned != "kitchen" {
                let printer_default = state2.printer_service
                    .find_default(cid, "order").await
                    .ok().flatten();
                let paper_width = printer_default.as_ref().map(|p| p.paper_width).unwrap_or(80);
                let printer_name = printer_default.as_ref().map(|p| p.system_name.clone());
                let result = match crate::print::pdf::build_full_receipt_pdf(
                    &order, &customer_name, &customer_phone, paper_width,
                ) {
                    Ok(bytes) => send_pdf_to_printer(&bytes, suffix, printer_name.as_deref()),
                    Err(e) => Err(format!("falha ao gerar PDF: {e}")),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = ui_weak2.upgrade() else { return };
                    match result {
                        Ok(()) => show_toast(&ui, "Comanda Enviada", "success"),
                        Err(e) => {
                            tracing::error!("Falha ao imprimir: {e}");
                            show_toast(&ui, "Falha ao imprimir (verifique impressora)", "error");
                        }
                    }
                });
                return;
            }

            // ── COZINHA (kind=kitchen) — roteamento por categoria ──
            // 1. Resolve a categoria de cada item (lookup em
            //    product_service). Itens sem produto cadastrado
            //    caem como `None` e vão pro fallback (default).
            let mut item_category: std::collections::HashMap<Uuid, Option<Uuid>> =
                std::collections::HashMap::new();
            for it in &order.items {
                if item_category.contains_key(&it.product_id) { continue; }
                let cat = state2.product_service
                    .find_by_id(cid, it.product_id).await
                    .ok().flatten()
                    .and_then(|p| p.category_id);
                item_category.insert(it.product_id, cat);
            }

            // 2. Carrega impressoras `kind=kitchen`. Se não houver
            //    nenhuma, cai no caminho antigo (1 PDF tudo, default
            //    do SO) — preserva compatibilidade.
            let kitchen_printers = state2.printer_service
                .find_by_kind(cid, "kitchen").await
                .unwrap_or_default();
            if kitchen_printers.is_empty() {
                let result = match crate::print::pdf::build_kitchen_receipt_pdf(
                    &order, &customer_name, &customer_phone, 80,
                ) {
                    Ok(bytes) => send_pdf_to_printer(&bytes, suffix, None),
                    Err(e) => Err(format!("falha ao gerar PDF: {e}")),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = ui_weak2.upgrade() else { return };
                    match result {
                        Ok(()) => show_toast(&ui, "Comanda Enviada", "success"),
                        Err(e) => {
                            tracing::error!("Falha ao imprimir: {e}");
                            show_toast(&ui, "Falha ao imprimir (verifique impressora)", "error");
                        }
                    }
                });
                return;
            }

            // 3. Roteia cada item para as impressoras que casam.
            //    Regras:
            //    - Impressora sem `category_ids` (catch-all) recebe TUDO.
            //    - Impressora com `category_ids` recebe itens cuja
            //      categoria está na lista.
            //    - Item órfão (não casou em nenhuma impressora) vai
            //      pra impressora `is_default = true` (se houver).
            //    - Um item PODE entrar em múltiplas impressoras
            //      (catch-all + estação específica). Esperado por design.
            let default_idx = kitchen_printers.iter().position(|p| p.is_default);
            let mut buckets: Vec<Vec<OrderItem>> =
                kitchen_printers.iter().map(|_| Vec::new()).collect();
            for it in &order.items {
                let cat = item_category.get(&it.product_id).copied().flatten();
                let mut routed = false;
                for (i, printer) in kitchen_printers.iter().enumerate() {
                    let matches = if printer.category_ids.is_empty() {
                        true
                    } else {
                        cat.is_some_and(|c| printer.category_ids.contains(&c))
                    };
                    if matches {
                        buckets[i].push(it.clone());
                        routed = true;
                    }
                }
                if !routed {
                    if let Some(def) = default_idx {
                        buckets[def].push(it.clone());
                    } else {
                        // Sem default — anexa à primeira impressora
                        // disponível para garantir que o item saia em
                        // algum lugar (melhor duplicar do que sumir).
                        buckets[0].push(it.clone());
                    }
                }
            }

            // 4. Gera e envia 1 PDF por impressora (apenas as que têm
            //    pelo menos 1 item). Acumula erros num Vec para o
            //    toast final mostrar a contagem.
            let mut errors: Vec<String> = Vec::new();
            let mut printed_count = 0;
            for (i, items) in buckets.iter().enumerate() {
                if items.is_empty() { continue; }
                let printer = &kitchen_printers[i];
                // Order parcial: clona o original mas com itens filtrados.
                let mut partial = order.clone();
                partial.items = items.clone();
                let pdf = crate::print::pdf::build_kitchen_receipt_pdf(
                    &partial, &customer_name, &customer_phone, printer.paper_width,
                );
                match pdf {
                    Ok(bytes) => {
                        match send_pdf_to_printer(&bytes, suffix, Some(&printer.system_name)) {
                            Ok(()) => printed_count += 1,
                            Err(e) => errors.push(format!("{}: {e}", printer.name)),
                        }
                    }
                    Err(e) => errors.push(format!("{}: PDF {e}", printer.name)),
                }
            }
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak2.upgrade() else { return };
                if errors.is_empty() && printed_count > 0 {
                    let msg = if printed_count == 1 {
                        "Comanda Enviada".to_string()
                    } else {
                        format!("{printed_count} comandas enviadas")
                    };
                    show_toast(&ui, &msg, "success");
                } else if !errors.is_empty() {
                    tracing::error!("Falhas ao imprimir: {errors:?}");
                    show_toast(&ui, "Falha em alguma impressora — verifique config", "error");
                } else {
                    show_toast(&ui, "Nenhum item para imprimir", "info");
                }
            });
        });
    });
}

/// Salva o PDF em arquivo temporário e envia para impressão.
///
/// Estratégia por SO:
/// - **Linux/macOS**: `lp` (CUPS). Com `printer_name`, usa
///   `lp -d <nome>`. CUPS tem filtro PDF nativo — qualquer impressora
///   registrada aceita.
/// - **Windows**: `Start-Process file.pdf -Verb PrintTo -ArgumentList "<nome>"`
///   delega ao handler de PDF do SO (geralmente Edge ou Reader).
///   Quando `printer_name` é `None`, usa `-Verb Print` (impressora padrão).
///
/// Se o spooler falhar (ex.: impressora offline, nome inválido), o erro
/// chega ao caller que mostra toast — o pedido não é perdido.
pub(crate) fn send_pdf_to_printer(
    bytes: &[u8],
    suffix: &str,
    printer_name: Option<&str>,
) -> Result<(), String> {
    let mut path = std::env::temp_dir();
    let name = format!(
        "letaf-{suffix}-{ts}.pdf",
        ts = chrono::Utc::now().format("%Y%m%d%H%M%S")
    );
    path.push(name);
    std::fs::write(&path, bytes).map_err(|e| format!("escrita do temp: {e}"))?;
    let status = if cfg!(target_os = "windows") {
        // `PrintTo` aceita o nome da impressora como argumento ao
        // handler associado a .pdf. Quando `printer_name` é None,
        // `Print` usa a impressora padrão.
        let cmd = match printer_name {
            Some(n) => format!(
                "Start-Process -FilePath '{}' -Verb PrintTo -ArgumentList '\"{}\"' -WindowStyle Hidden -Wait",
                path.display(), n
            ),
            None => format!(
                "Start-Process -FilePath '{}' -Verb Print -WindowStyle Hidden -Wait",
                path.display()
            ),
        };
        std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &cmd])
            .status()
    } else {
        let mut cmd = std::process::Command::new("lp");
        if let Some(n) = printer_name { cmd.arg("-d").arg(n); }
        cmd.arg(&path).status()
    }
    .map_err(|e| format!("spawn: {e}"))?;
    if !status.success() {
        return Err(format!("comando retornou {:?}", status.code()));
    }
    Ok(())
}

/// Usada pelo teste de impressão do cadastro de impressora —
/// gera texto plain (sem PDF) para validação rápida do canal.
pub(crate) fn send_to_default_printer(
    content: &str,
    suffix: &str,
    printer_name: Option<&str>,
) -> Result<(), String> {
    let mut path = std::env::temp_dir();
    let name = format!(
        "letaf-{suffix}-{ts}.txt",
        ts = chrono::Utc::now().format("%Y%m%d%H%M%S")
    );
    path.push(name);
    std::fs::write(&path, content.as_bytes())
        .map_err(|e| format!("escrita do temp: {e}"))?;
    let status = if cfg!(target_os = "windows") {
        let cmd = match printer_name {
            // Aspas simples no PowerShell evitam interpolação de `$` no
            // nome da impressora. Não tratamos `'` no nome — operadores
            // raramente cadastram impressora com aspas.
            Some(n) => format!(
                "Get-Content -LiteralPath '{}' | Out-Printer -Name '{}'",
                path.display(), n
            ),
            None => format!("Get-Content -LiteralPath '{}' | Out-Printer", path.display()),
        };
        std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &cmd])
            .status()
    } else {
        let mut cmd = std::process::Command::new("lp");
        if let Some(n) = printer_name { cmd.arg("-d").arg(n); }
        cmd.arg(&path).status()
    }
    .map_err(|e| format!("spawn: {e}"))?;
    if !status.success() {
        return Err(format!("comando retornou {:?}", status.code()));
    }
    Ok(())
}

/// Extrai a parte do endereço (`Rua, Nº, Bairro`) do `notes` que vem
/// no formato `[Tipo] Rua, Nº, Bairro | obs`. Devolve `None` quando o
/// `notes` não segue esse padrão.
pub(crate) fn extract_address_for_print(raw: &str) -> Option<String> {
    if !raw.starts_with('[') { return None; }
    let inner = if let Some(pipe) = raw.find(" | ") {
        &raw[..pipe]
    } else {
        raw
    };
    let close = inner.find(']')?;
    let addr = inner.get(close + 2..)?.trim();
    if addr.is_empty() { None } else { Some(addr.to_string()) }
}
