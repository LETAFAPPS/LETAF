//! PDFs das faturas da assinatura (documento A4).
//!
//! Dois documentos:
//! - [`build_invoice_pdf`] — recibo de UMA fatura.
//! - [`build_statement_pdf`] — extrato ÚNICO com as faturas dos
//!   últimos 12 meses (o "Baixar 12 meses" da tela Plano & Cobrança).
//!
//! Só o LAYOUT destes documentos mora aqui: cursor, escrita, divisor e
//! medida de texto vêm de [`super::text`] (fonte única, compartilhada
//! com a comanda).
//!
//! Texto nas fontes Type1 embutidas do PDF (Helvetica, encoding
//! WinAnsi) e a marca LETAF como imagem (ver [`super::brand`]) — sem
//! dependência de fontes/artes do SO. Coordenadas PDF têm origem no
//! canto inferior esquerdo; mantemos um cursor a partir do TOPO e
//! convertemos em [`Ctx::coord_y`], igual ao `print::pdf`.

use chrono::{Datelike, NaiveDate};
use printpdf::{BuiltinFont, IndirectFontRef, Mm, PdfDocument};

use letaf_core::subscription::model::{Invoice, InvoiceStatus};

use super::text::{
    advance, divider, text_width_mm, write_at, write_left, write_pair, Ctx, Page, DEFAULT_RULE_PT,
};
use crate::format::money_br;

const PAGE_W: f32 = 210.0; // A4
const PAGE_H: f32 = 297.0;
const MARGIN: f32 = 20.0;
/// Altura de uma linha de texto corrido.
const LINE: f32 = 7.0;

/// Colunas do extrato como frações da largura útil: (início, fim).
/// O rótulo e o valor de cada coluna são CENTRALIZADOS na mesma faixa,
/// o que mantém cabeçalho e dados visualmente alinhados.
const COLS: [(f32, f32); 5] = [
    (0.00, 0.16), // data
    (0.16, 0.33), // nº da fatura
    (0.33, 0.63), // descrição
    (0.63, 0.80), // situação
    (0.80, 1.00), // valor
];

/// Geometria fixa destes documentos (A4 retrato). O `Ctx` compartilhado
/// é usado sem estilo próprio (`()`): os tamanhos de fonte são fixos por
/// bloco de layout.
const PAGE: Page = Page { width_mm: PAGE_W, height_mm: PAGE_H, margin_mm: MARGIN };

/// Recibo de uma fatura.
pub fn build_invoice_pdf(inv: &Invoice, company_name: &str) -> Result<Vec<u8>, String> {
    let (doc, page, layer_idx) = PdfDocument::new(
        format!("Fatura {}", inv.number),
        Mm(PAGE_W),
        Mm(PAGE_H),
        "Layer 1",
    );
    let layer = doc.get_page(page).get_layer(layer_idx);
    let font = doc.add_builtin_font(BuiltinFont::Helvetica).map_err(|e| e.to_string())?;
    let bold = doc.add_builtin_font(BuiltinFont::HelveticaBold).map_err(|e| e.to_string())?;
    let mut ctx = Ctx { layer, font: &font, bold: &bold, page: PAGE, cursor_y: MARGIN, style: () };

    header(&mut ctx, company_name, "RECIBO DE FATURA");

    write_left(&ctx, &format!("Fatura {}", inv.number), 17.0, ctx.bold);
    advance(&mut ctx, LINE * 2.0);

    pair(&mut ctx, "Emissão", &inv.issued_at.format("%d/%m/%Y").to_string());
    pair(&mut ctx, "Descrição", &inv.description);
    pair(&mut ctx, "Forma de pagamento", method_label(inv));
    pair(&mut ctx, "Situação", status_label(inv.status));
    if let Some(paid) = inv.paid_at {
        pair(&mut ctx, "Pago em", &paid.format("%d/%m/%Y às %H:%M").to_string());
    }

    advance(&mut ctx, LINE * 0.9);
    rule(&ctx);
    advance(&mut ctx, LINE * 1.4);
    pair_strong(&mut ctx, "TOTAL", &money_br(inv.amount));

    footer_note(&mut ctx);
    doc.save_to_bytes().map_err(|e| e.to_string())
}

/// Extrato consolidado das faturas dos últimos 12 meses (arquivo único).
pub fn build_statement_pdf(
    invoices: &[Invoice],
    company_name: &str,
    today: NaiveDate,
) -> Result<Vec<u8>, String> {
    let cutoff = twelve_months_ago(today);
    let mut list: Vec<&Invoice> = invoices.iter().filter(|i| i.issued_at >= cutoff).collect();
    list.sort_by_key(|i| std::cmp::Reverse(i.issued_at));

    let (doc, page, layer_idx) = PdfDocument::new(
        "Faturas · últimos 12 meses",
        Mm(PAGE_W),
        Mm(PAGE_H),
        "Layer 1",
    );
    let layer = doc.get_page(page).get_layer(layer_idx);
    let font = doc.add_builtin_font(BuiltinFont::Helvetica).map_err(|e| e.to_string())?;
    let bold = doc.add_builtin_font(BuiltinFont::HelveticaBold).map_err(|e| e.to_string())?;
    let mut ctx = Ctx { layer, font: &font, bold: &bold, page: PAGE, cursor_y: MARGIN, style: () };

    header(&mut ctx, company_name, "FATURAS · ÚLTIMOS 12 MESES");

    write_left(
        &ctx,
        &format!(
            "Período: {} a {}",
            cutoff.format("%d/%m/%Y"),
            today.format("%d/%m/%Y")
        ),
        10.0,
        ctx.font,
    );
    advance(&mut ctx, LINE * 2.2);

    // Cabeçalho da tabela (centralizado sobre cada coluna).
    row(&mut ctx, ["DATA", "FATURA", "DESCRIÇÃO", "SITUAÇÃO", "VALOR"], 9.0, true);
    advance(&mut ctx, LINE * 0.8);
    rule(&ctx);
    advance(&mut ctx, LINE * 1.1);

    let mut total = rust_decimal::Decimal::ZERO;
    let mut paid_total = rust_decimal::Decimal::ZERO;
    for inv in &list {
        let date = inv.issued_at.format("%d/%m/%Y").to_string();
        let amount = money_br(inv.amount);
        let description = elide(&inv.description, 34);
        row(
            &mut ctx,
            [&date, &inv.number, &description, status_label(inv.status), &amount],
            9.5,
            false,
        );
        advance(&mut ctx, LINE * 1.15);
        total += inv.amount;
        if inv.status == InvoiceStatus::Paid {
            paid_total += inv.amount;
        }
    }
    if list.is_empty() {
        write_left(&ctx, "Nenhuma fatura no período.", 10.0, ctx.font);
        advance(&mut ctx, LINE);
    }

    advance(&mut ctx, LINE * 0.6);
    rule(&ctx);
    advance(&mut ctx, LINE * 1.4);
    pair(&mut ctx, "Faturas no período", &list.len().to_string());
    pair(&mut ctx, "Total pago", &money_br(paid_total));
    advance(&mut ctx, LINE * 0.3);
    pair_strong(&mut ctx, "TOTAL EMITIDO", &money_br(total));

    footer_note(&mut ctx);
    doc.save_to_bytes().map_err(|e| e.to_string())
}

/// Primeiro dia do mês 11 meses atrás — janela de 12 meses corridos.
fn twelve_months_ago(today: NaiveDate) -> NaiveDate {
    let months = today.year() * 12 + today.month() as i32 - 1 - 11;
    let (y, m) = (months.div_euclid(12), months.rem_euclid(12) as u32 + 1);
    NaiveDate::from_ymd_opt(y, m, 1).unwrap_or(today)
}

// ── Blocos ───────────────────────────────────────────────────────

/// Cabeçalho: estrela + wordmark do LETAF, nome do estabelecimento e
/// título do documento, fechando com uma divisória.
fn header(ctx: &mut Ctx<'_>, company_name: &str, title: &str) {
    const STAR_W: f32 = 11.0;
    const GAP: f32 = 4.5;
    const WORD_W: f32 = 30.0;

    let star = super::brand::star(180);
    let word = super::brand::wordmark(520);
    let star_h = star.as_ref().map(|s| s.height_for_width(STAR_W)).unwrap_or(0.0);
    let word_h = word.as_ref().map(|w| w.height_for_width(WORD_W)).unwrap_or(0.0);
    let brand_h = star_h.max(word_h);

    if let Some(star) = star {
        let dy = (brand_h - star_h) / 2.0;
        star.draw(&ctx.layer, MARGIN, ctx.cursor_y + dy, STAR_W, PAGE_H);
    }
    if let Some(word) = word {
        let dy = (brand_h - word_h) / 2.0;
        word.draw(&ctx.layer, MARGIN + STAR_W + GAP, ctx.cursor_y + dy, WORD_W, PAGE_H);
    }
    // Sem as artes (falha de decode), o nome textual segura o layout.
    if brand_h <= 0.0 {
        write_left(ctx, "LETAF", 20.0, ctx.bold);
        advance(ctx, LINE);
    } else {
        advance(ctx, brand_h + LINE * 1.1);
    }

    write_left(ctx, company_name, 11.5, ctx.font);
    advance(ctx, LINE * 1.0);
    write_left(ctx, title, 11.0, ctx.bold);
    advance(ctx, LINE * 0.9);
    rule(ctx);
    advance(ctx, LINE * 1.8);
}

fn footer_note(ctx: &mut Ctx<'_>) {
    advance(ctx, LINE * 2.4);
    write_left(
        ctx,
        "Documento gerado pelo LETAF — não possui valor fiscal.",
        9.0,
        ctx.font,
    );
}

/// Só a FORMA de pagamento (PIX/Cartão) — o documento não expõe os
/// dígitos do cartão.
fn method_label(inv: &Invoice) -> &'static str {
    match inv.method_kind.as_str() {
        "pix" => "PIX",
        "card" | "visa" => "Cartão",
        _ => "—",
    }
}

fn status_label(s: InvoiceStatus) -> &'static str {
    match s {
        InvoiceStatus::Paid => "Pago",
        InvoiceStatus::Pending => "Pendente",
        InvoiceStatus::Failed => "Falhou",
    }
}

// ── Linhas do layout ─────────────────────────────────────────────

/// Linha "rótulo ......... valor" (valor alinhado à direita).
fn pair(ctx: &mut Ctx<'_>, label: &str, value: &str) {
    write_pair(ctx, label, value, 10.5, ctx.font);
    advance(ctx, LINE);
}

fn pair_strong(ctx: &mut Ctx<'_>, label: &str, value: &str) {
    write_pair(ctx, label, value, 13.0, ctx.bold);
    advance(ctx, LINE * 1.2);
}

/// Linha da tabela do extrato: cada célula CENTRALIZADA na sua coluna
/// (mesma regra do cabeçalho, então os títulos ficam alinhados com as
/// informações). `bold` escolhe a fonte do próprio `ctx` — evita
/// emprestar `ctx` imutável e mutável ao mesmo tempo.
fn row(ctx: &mut Ctx<'_>, cells: [&str; 5], pt: f32, bold: bool) {
    let font: &IndirectFontRef = if bold { ctx.bold } else { ctx.font };
    let content = ctx.content_width_mm();
    for (cell, (from, to)) in cells.iter().zip(COLS.iter()) {
        let x0 = MARGIN + content * from;
        let x1 = MARGIN + content * to;
        let w = text_width_mm(cell, pt);
        let x = (x0 + (x1 - x0 - w) / 2.0).max(x0);
        write_at(ctx, cell, x, pt, font);
    }
}

/// Divisor destes documentos: espessura padrão do PDF, sem respiro
/// automático (cada bloco decide o espaço depois da linha).
fn rule(ctx: &Ctx<'_>) {
    divider(ctx, DEFAULT_RULE_PT);
}

fn elide(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{cut}…")
}

/// Nome de arquivo sugerido para o recibo de uma fatura.
pub fn invoice_file_name(inv: &Invoice) -> String {
    format!("fatura-{}.pdf", inv.number.replace(['/', ' '], "-"))
}

/// Nome de arquivo sugerido para o extrato de 12 meses.
pub fn statement_file_name(today: NaiveDate) -> String {
    format!("faturas-12-meses-{}.pdf", today.format("%Y-%m"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn janela_de_12_meses_comeca_no_primeiro_dia_do_mes() {
        let today = NaiveDate::from_ymd_opt(2026, 7, 30).unwrap();
        assert_eq!(
            twelve_months_ago(today),
            NaiveDate::from_ymd_opt(2025, 8, 1).unwrap()
        );
    }

    #[test]
    fn janela_de_12_meses_vira_o_ano_em_janeiro() {
        let today = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        assert_eq!(
            twelve_months_ago(today),
            NaiveDate::from_ymd_opt(2025, 2, 1).unwrap()
        );
    }

    #[test]
    fn descricao_longa_e_encurtada_com_reticencias() {
        assert_eq!(elide("Plano Pro mensal", 8), "Plano P…");
        assert_eq!(elide("Plano", 8), "Plano");
    }
}
