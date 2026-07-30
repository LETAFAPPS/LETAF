//! PDFs das faturas da assinatura (documento A4).
//!
//! Dois documentos:
//! - [`build_invoice_pdf`] — recibo de UMA fatura.
//! - [`build_statement_pdf`] — extrato ÚNICO com as faturas dos
//!   últimos 12 meses (o "Baixar Todos" da tela de Plano & Cobrança).
//!
//! Usa as fontes Type1 embutidas do PDF (Helvetica), encoding WinAnsi —
//! sem dependência de fontes do SO. Coordenadas PDF têm origem no canto
//! inferior esquerdo; mantemos um cursor `y` a partir do TOPO e
//! convertemos em [`Ctx::coord_y`], espelhando o `print::pdf`.

use chrono::{Datelike, NaiveDate};
use printpdf::{
    BuiltinFont, IndirectFontRef, Line, Mm, PdfDocument, PdfLayerReference, Point,
};

use letaf_core::subscription::model::{Invoice, InvoiceStatus};

use crate::format::money_br;

const PAGE_W: f32 = 210.0; // A4
const PAGE_H: f32 = 297.0;
const MARGIN: f32 = 18.0;
const LINE: f32 = 6.0;

struct Ctx<'a> {
    layer: PdfLayerReference,
    font: &'a IndirectFontRef,
    bold: &'a IndirectFontRef,
    /// Cursor em mm a partir do TOPO da página.
    y: f32,
}

impl Ctx<'_> {
    fn coord_y(&self, from_top: f32) -> Mm {
        Mm(PAGE_H - from_top)
    }
    fn content_w(&self) -> f32 {
        PAGE_W - MARGIN * 2.0
    }
}

/// Recibo de uma fatura.
pub fn build_invoice_pdf(inv: &Invoice, company_name: &str) -> Result<Vec<u8>, String> {
    let (doc, page, layer_idx) =
        PdfDocument::new(format!("Fatura {}", inv.number), Mm(PAGE_W), Mm(PAGE_H), "Layer 1");
    let layer = doc.get_page(page).get_layer(layer_idx);
    let font = doc.add_builtin_font(BuiltinFont::Helvetica).map_err(|e| e.to_string())?;
    let bold = doc.add_builtin_font(BuiltinFont::HelveticaBold).map_err(|e| e.to_string())?;
    let mut ctx = Ctx { layer, font: &font, bold: &bold, y: MARGIN };

    header(&mut ctx, company_name, "RECIBO DE FATURA");
    write_left(&ctx, &format!("Fatura {}", inv.number), 16.0, ctx.bold);
    advance(&mut ctx, LINE * 1.6);

    pair(&mut ctx, "Emissão", &inv.issued_at.format("%d/%m/%Y").to_string());
    pair(&mut ctx, "Descrição", &inv.description);
    pair(&mut ctx, "Forma de pagamento", &method_label(inv));
    pair(&mut ctx, "Situação", status_label(inv.status));
    if let Some(paid) = inv.paid_at {
        pair(&mut ctx, "Pago em", &paid.format("%d/%m/%Y às %H:%M").to_string());
    }

    advance(&mut ctx, LINE * 0.6);
    divider(&mut ctx);
    advance(&mut ctx, LINE * 0.8);
    pair_strong(&mut ctx, "TOTAL", &money_br(inv.amount));

    advance(&mut ctx, LINE * 2.0);
    write_left(
        &ctx,
        "Documento gerado pelo LETAF — não possui valor fiscal.",
        9.0,
        ctx.font,
    );
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

    let (doc, page, layer_idx) =
        PdfDocument::new("Faturas · últimos 12 meses", Mm(PAGE_W), Mm(PAGE_H), "Layer 1");
    let layer = doc.get_page(page).get_layer(layer_idx);
    let font = doc.add_builtin_font(BuiltinFont::Helvetica).map_err(|e| e.to_string())?;
    let bold = doc.add_builtin_font(BuiltinFont::HelveticaBold).map_err(|e| e.to_string())?;
    let mut ctx = Ctx { layer, font: &font, bold: &bold, y: MARGIN };

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
    advance(&mut ctx, LINE * 1.6);

    // Cabeçalho da tabela.
    row(&mut ctx, "DATA", "FATURA", "DESCRIÇÃO", "SITUAÇÃO", "VALOR", 9.0, true);
    advance(&mut ctx, LINE * 0.5);
    divider(&mut ctx);
    advance(&mut ctx, LINE * 0.7);

    let mut total = rust_decimal::Decimal::ZERO;
    let mut paid_total = rust_decimal::Decimal::ZERO;
    for inv in &list {
        row(
            &mut ctx,
            &inv.issued_at.format("%d/%m/%Y").to_string(),
            &inv.number,
            &inv.description,
            status_label(inv.status),
            &money_br(inv.amount),
            9.5,
            false,
        );
        advance(&mut ctx, LINE);
        total += inv.amount;
        if inv.status == InvoiceStatus::Paid {
            paid_total += inv.amount;
        }
    }
    if list.is_empty() {
        write_left(&ctx, "Nenhuma fatura no período.", 10.0, ctx.font);
        advance(&mut ctx, LINE);
    }

    advance(&mut ctx, LINE * 0.4);
    divider(&mut ctx);
    advance(&mut ctx, LINE * 0.9);
    pair(&mut ctx, &format!("{} faturas no período", list.len()), "");
    pair(&mut ctx, "Total pago", &money_br(paid_total));
    pair_strong(&mut ctx, "TOTAL EMITIDO", &money_br(total));

    advance(&mut ctx, LINE * 1.6);
    write_left(
        &ctx,
        "Documento gerado pelo LETAF — não possui valor fiscal.",
        9.0,
        ctx.font,
    );
    doc.save_to_bytes().map_err(|e| e.to_string())
}

/// Primeiro dia do mês 11 meses atrás — janela de 12 meses corridos.
fn twelve_months_ago(today: NaiveDate) -> NaiveDate {
    let months = today.year() * 12 + today.month() as i32 - 1 - 11;
    let (y, m) = (months.div_euclid(12), months.rem_euclid(12) as u32 + 1);
    NaiveDate::from_ymd_opt(y, m, 1).unwrap_or(today)
}

// ── Blocos ───────────────────────────────────────────────────────

fn header(ctx: &mut Ctx, company_name: &str, title: &str) {
    write_left(ctx, "LETAF", 20.0, ctx.bold);
    advance(ctx, LINE * 0.9);
    write_left(ctx, company_name, 11.0, ctx.font);
    advance(ctx, LINE * 0.9);
    write_left(ctx, title, 11.0, ctx.bold);
    advance(ctx, LINE * 0.8);
    divider(ctx);
    advance(ctx, LINE * 1.1);
}

fn method_label(inv: &Invoice) -> String {
    let kind = match inv.method_kind.as_str() {
        "pix" => "PIX",
        "card" | "visa" => "Cartão",
        other if !other.is_empty() => other,
        _ => "—",
    };
    if inv.method_label.is_empty() {
        kind.to_string()
    } else {
        format!("{kind} · {}", inv.method_label)
    }
}

fn status_label(s: InvoiceStatus) -> &'static str {
    match s {
        InvoiceStatus::Paid => "Pago",
        InvoiceStatus::Pending => "Pendente",
        InvoiceStatus::Failed => "Falhou",
    }
}

// ── Primitivas de escrita ────────────────────────────────────────

fn write_left(ctx: &Ctx, text: &str, pt: f32, font: &IndirectFontRef) {
    let y = ctx.coord_y(ctx.y + pt_to_mm(pt) * 0.75);
    ctx.layer.use_text(sanitize(text), pt, Mm(MARGIN), y, font);
}

fn write_at(ctx: &Ctx, text: &str, x_mm: f32, pt: f32, font: &IndirectFontRef) {
    let y = ctx.coord_y(ctx.y + pt_to_mm(pt) * 0.75);
    ctx.layer.use_text(sanitize(text), pt, Mm(x_mm), y, font);
}

/// Linha "rótulo ......... valor" (valor alinhado à direita).
fn pair(ctx: &mut Ctx, label: &str, value: &str) {
    write_left(ctx, label, 10.0, ctx.font);
    if !value.is_empty() {
        let w = text_width_mm(value, 10.0);
        write_at(ctx, value, PAGE_W - MARGIN - w, 10.0, ctx.font);
    }
    advance(ctx, LINE);
}

fn pair_strong(ctx: &mut Ctx, label: &str, value: &str) {
    write_left(ctx, label, 13.0, ctx.bold);
    let w = text_width_mm(value, 13.0);
    write_at(ctx, value, PAGE_W - MARGIN - w, 13.0, ctx.bold);
    advance(ctx, LINE * 1.2);
}

/// Linha da tabela do extrato (5 colunas; valor à direita).
/// `bold` escolhe a fonte do próprio `ctx` — evita emprestar `ctx`
/// imutável e mutável ao mesmo tempo.
#[allow(clippy::too_many_arguments)]
fn row(
    ctx: &mut Ctx,
    date: &str,
    number: &str,
    description: &str,
    status: &str,
    amount: &str,
    pt: f32,
    bold: bool,
) {
    let font: &IndirectFontRef = if bold { ctx.bold } else { ctx.font };
    let content = ctx.content_w();
    write_at(ctx, date, MARGIN, pt, font);
    write_at(ctx, number, MARGIN + content * 0.16, pt, font);
    write_at(ctx, &elide(description, 46), MARGIN + content * 0.33, pt, font);
    write_at(ctx, status, MARGIN + content * 0.70, pt, font);
    let w = text_width_mm(amount, pt);
    write_at(ctx, amount, PAGE_W - MARGIN - w, pt, font);
}

fn divider(ctx: &mut Ctx) {
    let y = ctx.coord_y(ctx.y);
    let line = Line {
        points: vec![
            (Point::new(Mm(MARGIN), y), false),
            (Point::new(Mm(PAGE_W - MARGIN), y), false),
        ],
        is_closed: false,
    };
    ctx.layer.add_line(line);
}

fn advance(ctx: &mut Ctx, delta_mm: f32) {
    ctx.y += delta_mm;
}

fn pt_to_mm(pt: f32) -> f32 {
    pt * 25.4 / 72.0
}

/// Largura aproximada em mm (Helvetica ≈ 0.5em por caractere).
fn text_width_mm(text: &str, pt: f32) -> f32 {
    pt_to_mm(pt) * 0.5 * text.chars().count() as f32
}

fn elide(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{cut}…")
}

/// WinAnsi (CP1252) não cobre alguns símbolos; troca pelos equivalentes.
fn sanitize(text: &str) -> String {
    text.replace('—', "-").replace('…', "...").replace('·', "-")
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
}
