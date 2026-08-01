//! Geração de PDFs das comandas (cliente + cozinha).
//!
//! Só o LAYOUT da comanda mora aqui: cursor, escrita, divisor, medida e
//! quebra de texto vêm de [`super::text`] (fonte única, compartilhada
//! com a fatura).
//!
//! Usa as 14 fontes Type1 built-in do PDF (Helvetica/Helvetica-Bold) —
//! nenhum TTF embutido, sem dependência de fontes do SO. Encoding
//! WinAnsi (CP1252) cobre acentos do português e `R$` (importantes nas
//! comandas).
//!
//! Convenção de coordenadas PDF: origem no canto inferior esquerdo,
//! Y aumenta para cima. Para facilitar a leitura, mantemos um cursor
//! `y` que decrementamos a cada linha — espelha a escrita de cima
//! para baixo.

use letaf_core::order::model::{DeliveryType, Order, OrderItem};
use printpdf::{BuiltinFont, Line, Mm, PdfDocument, Point};
use rust_decimal::prelude::ToPrimitive;

use super::text::{
    advance, divider, wrap_text, write_centered, write_left, write_left_indented, write_pair, Ctx,
    Page, HAIRLINE_PT,
};
use crate::format::{format_order_date, format_order_time};
use crate::ui::orders::{
    extract_address_for_print, format_addons_summary, format_elapsed_since, format_qty,
    parse_address_parts, strip_address_prefix,
};

/// Estilo tipográfico — escolhido para casar com os tamanhos do
/// `ReceiptModal` em escala para papel térmico (58/80mm). Em A4 isso
/// dá sensação de "ticket impresso" — bom para conferência manual.
struct Style {
    page_width_mm: f32,
    margin_mm: f32,
    /// Espaço entre linhas em mm (vai para o `line_height_factor` quando
    /// quebramos texto em múltiplas linhas).
    line_height_mm: f32,
    /// Tamanhos de fonte em pontos PDF (1 pt = 1/72 polegada).
    title_pt: f32,
    modality_pt: f32,
    customer_pt: f32,
    body_strong_pt: f32,
    body_pt: f32,
    addons_pt: f32,
    section_pt: f32,
    total_pt: f32,
}

impl Style {
    fn for_paper(paper_width: i32) -> Self {
        // Térmica 58mm: papel estreito, fontes menores; 80mm: padrão;
        // 210mm: folha normal (A4), com margens e corpo de folha.
        // Outros valores caem no preset 80mm (sem-fronteiras).
        match paper_width {
            letaf_core::printer::model::A4_PAPER_WIDTH => Self {
                page_width_mm: 210.0,
                margin_mm: 18.0,
                line_height_mm: 6.0,
                title_pt: 18.0,
                modality_pt: 14.0,
                customer_pt: 14.0,
                body_strong_pt: 12.0,
                body_pt: 11.0,
                addons_pt: 9.5,
                section_pt: 9.5,
                total_pt: 16.0,
            },
            58 => Self {
                page_width_mm: 58.0,
                margin_mm: 3.0,
                line_height_mm: 3.5,
                title_pt: 11.0,
                modality_pt: 10.0,
                customer_pt: 10.5,
                body_strong_pt: 8.5,
                body_pt: 8.0,
                addons_pt: 7.0,
                section_pt: 7.0,
                total_pt: 11.0,
            },
            _ => Self {
                page_width_mm: 80.0,
                margin_mm: 5.0,
                line_height_mm: 4.5,
                title_pt: 14.0,
                modality_pt: 12.0,
                customer_pt: 13.0,
                body_strong_pt: 10.5,
                body_pt: 10.0,
                addons_pt: 8.5,
                section_pt: 8.5,
                total_pt: 13.0,
            },
        }
    }

    /// Geometria da página para as primitivas de [`super::text`] — a
    /// altura só é conhecida depois da estimativa do conteúdo.
    fn page(&self, height_mm: f32) -> Page {
        Page { width_mm: self.page_width_mm, height_mm, margin_mm: self.margin_mm }
    }

    /// Altura final da página. Bobina térmica é contínua — a página tem
    /// exatamente a altura do conteúdo. Folha normal (A4) tem altura
    /// fixa de 297 mm; se o conteúdo passar disso, a página cresce para
    /// não cortar nada.
    fn page_height_mm(&self, content_mm: f32) -> f32 {
        if self.page_width_mm >= 210.0 {
            content_mm.max(297.0)
        } else {
            content_mm
        }
    }
}

/// Contexto da comanda: o `Ctx` compartilhado carregando o `Style` da
/// largura do papel como estilo do documento.
type ReceiptCtx<'a> = Ctx<'a, &'a Style>;

/// Gera o PDF da comanda COMPLETA do cliente.
pub fn build_full_receipt_pdf(
    order: &Order,
    customer_name: &str,
    customer_phone: &str,
    paper_width: i32,
) -> Result<Vec<u8>, String> {
    let style = Style::for_paper(paper_width);
    let page_height_mm = style.page_height_mm(estimate_height_full(order, customer_phone, &style));
    let (doc, page1, layer1) = PdfDocument::new(
        "Cupom",
        Mm(style.page_width_mm),
        Mm(page_height_mm),
        "Layer 1",
    );
    let layer = doc.get_page(page1).get_layer(layer1);
    let font = doc.add_builtin_font(BuiltinFont::Helvetica).map_err(|e| e.to_string())?;
    let font_bold = doc.add_builtin_font(BuiltinFont::HelveticaBold).map_err(|e| e.to_string())?;
    let mut ctx = ReceiptCtx {
        layer,
        font: &font,
        bold: &font_bold,
        page: style.page(page_height_mm),
        cursor_y: style.margin_mm,
        style: &style,
    };
    render_header(&mut ctx, order);
    render_modality(&mut ctx, order);
    render_divider(&mut ctx);
    render_customer_block(&mut ctx, customer_name, customer_phone, order);
    render_divider(&mut ctx);
    render_items_section(&mut ctx, &order.items, /*show_prices*/ true);
    render_divider(&mut ctx);
    render_totals(&mut ctx, order);
    render_notes(&mut ctx, order);
    doc.save_to_bytes().map_err(|e| e.to_string())
}

/// Gera o PDF da comanda da COZINHA (sem valores, foco nos itens).
pub fn build_kitchen_receipt_pdf(
    order: &Order,
    customer_name: &str,
    customer_phone: &str,
    paper_width: i32,
) -> Result<Vec<u8>, String> {
    let style = Style::for_paper(paper_width);
    let page_height_mm =
        style.page_height_mm(estimate_height_kitchen(order, customer_phone, &style));
    let (doc, page1, layer1) = PdfDocument::new(
        "Comanda",
        Mm(style.page_width_mm),
        Mm(page_height_mm),
        "Layer 1",
    );
    let layer = doc.get_page(page1).get_layer(layer1);
    let font = doc.add_builtin_font(BuiltinFont::Helvetica).map_err(|e| e.to_string())?;
    let font_bold = doc.add_builtin_font(BuiltinFont::HelveticaBold).map_err(|e| e.to_string())?;
    let mut ctx = ReceiptCtx {
        layer,
        font: &font,
        bold: &font_bold,
        page: style.page(page_height_mm),
        cursor_y: style.margin_mm,
        style: &style,
    };
    render_header_kitchen(&mut ctx, order);
    render_modality(&mut ctx, order);
    render_divider(&mut ctx);
    render_customer_block(&mut ctx, customer_name, customer_phone, order);
    render_divider(&mut ctx);
    render_items_section(&mut ctx, &order.items, /*show_prices*/ false);
    render_notes(&mut ctx, order);
    doc.save_to_bytes().map_err(|e| e.to_string())
}

// ── Renderers ─────────────────────────────────────────────────────

fn render_header(ctx: &mut ReceiptCtx<'_>, order: &Order) {
    let title = format!("PEDIDO #{:04}", order.number);
    write_centered(ctx, &title, ctx.style.title_pt, ctx.bold);
    advance(ctx, ctx.style.line_height_mm * 1.0);
    let date = format_order_date(order.base.created_at);
    let time = format_order_time(order.base.created_at);
    write_centered(ctx, &format!("{date} · {time}"), ctx.style.body_pt, ctx.font);
    advance(ctx, ctx.style.line_height_mm * 1.4);
}

fn render_header_kitchen(ctx: &mut ReceiptCtx<'_>, order: &Order) {
    let title = format!("COZINHA · #{:04}", order.number);
    write_centered(ctx, &title, ctx.style.title_pt, ctx.bold);
    advance(ctx, ctx.style.line_height_mm * 1.0);
    let time = format_order_time(order.base.created_at);
    let elapsed = format_elapsed_since(order.base.created_at, &order.status);
    let elapsed_show = if elapsed.is_empty() { "agora".to_string() } else { elapsed };
    write_centered(ctx, &format!("{time}   ·   {elapsed_show}"), ctx.style.body_pt, ctx.font);
    advance(ctx, ctx.style.line_height_mm * 1.4);
}

fn render_modality(ctx: &mut ReceiptCtx<'_>, order: &Order) {
    let label = match order.delivery_type {
        DeliveryType::Delivery => "ENTREGA",
        DeliveryType::Pickup => "RETIRADA",
    };
    write_centered(ctx, label, ctx.style.modality_pt, ctx.bold);
    advance(ctx, ctx.style.line_height_mm * 1.2);
}

fn render_customer_block(ctx: &mut ReceiptCtx<'_>, name: &str, phone: &str, order: &Order) {
    write_left(ctx, name, ctx.style.customer_pt, ctx.bold);
    advance(ctx, ctx.style.line_height_mm);
    if !phone.is_empty() {
        write_left(ctx, phone, ctx.style.body_strong_pt, ctx.bold);
        advance(ctx, ctx.style.line_height_mm);
    }
    let addr = order.notes.as_deref().and_then(extract_address_for_print);
    if let Some(addr_raw) = addr {
        let (street, number, neigh, _apt) = parse_address_parts(&addr_raw);
        let formatted = if !street.is_empty() && !number.is_empty() && !neigh.is_empty() {
            format!("{street}, {number} — {neigh}")
        } else {
            addr_raw
        };
        for line in wrap_text(&formatted, ctx.content_width_mm(), ctx.style.body_strong_pt) {
            write_left(ctx, &line, ctx.style.body_strong_pt, ctx.bold);
            advance(ctx, ctx.style.line_height_mm);
        }
    }
}

fn render_items_section(ctx: &mut ReceiptCtx<'_>, items: &[OrderItem], show_prices: bool) {
    write_left(ctx, "ITENS", ctx.style.section_pt, ctx.bold);
    advance(ctx, ctx.style.line_height_mm * 1.2);
    for it in items {
        render_item(ctx, it, show_prices);
    }
}

fn render_item(ctx: &mut ReceiptCtx<'_>, it: &OrderItem, show_prices: bool) {
    let qty = format_qty(it.quantity);
    let qty_label = format!("x{qty}");
    if show_prices {
        // "x1  Brahma 350ml ...... R$ 19.00"
        let left = format!("{qty_label}  {}", it.product_name);
        let right = format!("R$ {:.2}", it.subtotal);
        write_pair(ctx, &left, &right, ctx.style.body_strong_pt, ctx.bold);
        advance(ctx, ctx.style.line_height_mm);
        // Item com desconto de produto (snapshot da venda): mostra o
        // preço de tabela e o desconto para a conta ficar transparente.
        if let Some(list) = it.list_unit_price.filter(|lp| *lp > it.unit_price) {
            let qty_dec = letaf_core::money::qty(it.quantity);
            let list_total = letaf_core::money::round2(list * qty_dec);
            let discount = letaf_core::money::round2(list_total - it.subtotal);
            let line = format!("de R$ {list_total:.2} · desconto − R$ {discount:.2}");
            write_left_indented(ctx, &line, 6.0, ctx.style.addons_pt, ctx.font);
            advance(ctx, ctx.style.line_height_mm * 0.85);
        }
    } else {
        // Cozinha: sem preço.
        let left = format!("{qty_label}  {}", it.product_name);
        write_left(ctx, &left, ctx.style.body_strong_pt, ctx.bold);
        advance(ctx, ctx.style.line_height_mm);
    }
    let summary = format_addons_summary(it.addons_json.as_deref());
    if !summary.is_empty() {
        // Indenta com a largura aproximada de "x1  " (~6mm).
        let indent_mm = 6.0;
        let avail = ctx.content_width_mm() - indent_mm;
        for line in wrap_text(&summary, avail, ctx.style.addons_pt) {
            write_left_indented(ctx, &line, indent_mm, ctx.style.addons_pt, ctx.font);
            advance(ctx, ctx.style.line_height_mm * 0.85);
        }
    }
    advance(ctx, ctx.style.line_height_mm * 0.3);
}

fn render_totals(ctx: &mut ReceiptCtx<'_>, order: &Order) {
    let subtotal = order.items.iter().map(|i| i.subtotal).sum::<rust_decimal::Decimal>().to_f64().unwrap_or(0.0);
    write_pair(ctx, "Subtotal", &format!("R$ {:.2}", subtotal),
        ctx.style.body_pt, ctx.font);
    advance(ctx, ctx.style.line_height_mm);
    let fee_label = if order.delivery_type == DeliveryType::Delivery { "Grátis" } else { "—" };
    write_pair(ctx, "Taxa de entrega", fee_label, ctx.style.body_pt, ctx.font);
    advance(ctx, ctx.style.line_height_mm);
    // Desconto: impresso sempre que houver (cupom OU desconto direto de PDV) —
    // senão a conta não fecha com o TOTAL. A linha "Cupom" só aparece se houver
    // código.
    if order.discount_amount > rust_decimal::Decimal::ZERO {
        if let Some(code) = order.coupon_code.as_deref().filter(|c| !c.is_empty()) {
            write_pair(ctx, "Cupom", code, ctx.style.body_pt, ctx.font);
            advance(ctx, ctx.style.line_height_mm);
        }
        write_pair(ctx, "Desconto",
            &format!("− R$ {:.2}", order.discount_amount),
            ctx.style.body_pt, ctx.font);
        advance(ctx, ctx.style.line_height_mm);
    }
    // Acréscimo (taxa/ajuste do PDV): o TOTAL já o reflete, então precisa
    // aparecer para o papel bater matematicamente.
    if order.additional_amount > rust_decimal::Decimal::ZERO {
        write_pair(ctx, "Acréscimo",
            &format!("+ R$ {:.2}", order.additional_amount),
            ctx.style.body_pt, ctx.font);
        advance(ctx, ctx.style.line_height_mm);
    }
    advance(ctx, ctx.style.line_height_mm * 0.3);
    render_divider(ctx);
    write_pair(ctx, "TOTAL", &format!("R$ {:.2}", order.total),
        ctx.style.total_pt, ctx.bold);
    advance(ctx, ctx.style.line_height_mm * 1.4);
}

fn render_notes(ctx: &mut ReceiptCtx<'_>, order: &Order) {
    let free = order.notes.as_deref().map(strip_address_prefix).unwrap_or_default();
    if free.is_empty() { return; }
    advance(ctx, ctx.style.line_height_mm * 0.6);

    // Layout do card de Observações — espelha o `ReceiptModal` Slint:
    // Rectangle com borda 1px (border_radius não é suportado em
    // `Line is_closed` da printpdf, então fica retangular reto), label
    // "Observações" em destaque e texto livre embaixo, tudo com padding.
    let inner_pad = 3.0_f32;   // mm — padding interno do card
    let avail_text_mm = ctx.content_width_mm() - inner_pad * 2.0;
    let lines = wrap_text(&free, avail_text_mm, ctx.style.body_pt);
    let label_h = ctx.style.line_height_mm;
    let lines_h = lines.len() as f32 * ctx.style.line_height_mm;
    let card_h = inner_pad * 2.0 + label_h + lines_h;

    // Desenha o retângulo (Line fechado com 4 vértices) — borda preta
    // 0.3 pt para casar com os divisores horizontais da mesma comanda.
    let top_y = ctx.coord_y(ctx.cursor_y);
    let bottom_y = ctx.coord_y(ctx.cursor_y + card_h);
    let x_left = ctx.page.margin_mm;
    let x_right = ctx.page.width_mm - ctx.page.margin_mm;
    let card = Line {
        points: vec![
            (Point::new(Mm(x_left), top_y), false),
            (Point::new(Mm(x_right), top_y), false),
            (Point::new(Mm(x_right), bottom_y), false),
            (Point::new(Mm(x_left), bottom_y), false),
        ],
        is_closed: true,
    };
    ctx.layer.set_outline_thickness(HAIRLINE_PT);
    ctx.layer.add_line(card);

    // Escreve o conteúdo respeitando o padding interno. Avançamos
    // primeiro `inner_pad` (margem superior) e indentamos por
    // `inner_pad` à esquerda.
    advance(ctx, inner_pad);
    write_left_indented(ctx, "Observações", inner_pad, ctx.style.section_pt, ctx.bold);
    advance(ctx, label_h);
    for line in lines {
        write_left_indented(ctx, &line, inner_pad, ctx.style.body_pt, ctx.font);
        advance(ctx, ctx.style.line_height_mm);
    }
    // Margem inferior (não acumula com o próximo bloco — render_notes
    // é a última seção do PDF, mas mantemos por consistência).
    advance(ctx, inner_pad);
}

/// Divisor da comanda: linha fina (casa com a moldura do card de
/// observações) mais o respiro da própria comanda.
fn render_divider(ctx: &mut ReceiptCtx<'_>) {
    divider(ctx, HAIRLINE_PT);
    advance(ctx, ctx.style.line_height_mm * 0.9);
}

// ── Estimativas de altura ─────────────────────────────────────────

/// Estima a altura total da página em mm para a comanda completa.
/// Necessário porque criamos a página com tamanho fixo no
/// `PdfDocument::new` — não há fluxo dinâmico. Subestimar = corte;
/// superestimar = papel sobrando ao final. Adicionamos margem
/// generosa (line_height × 4) para evitar corte em casos extremos.
fn estimate_height_full(order: &Order, phone: &str, style: &Style) -> f32 {
    let mut lh = 0.0_f32;
    lh += 2.0; // header (2 linhas)
    lh += 1.2; // modalidade
    lh += 1.0; // divisor
    lh += 1.0; // cliente
    if !phone.is_empty() { lh += 1.0; } // telefone
    lh += 2.0; // endereço (até 2 linhas)
    lh += 1.0; // divisor
    lh += 1.2; // "ITENS"
    for it in &order.items {
        lh += 1.0;
        // adicional: até 2 linhas
        if it.addons_json.as_deref().filter(|s| !s.is_empty()).is_some() {
            lh += 1.7;
        }
        lh += 0.3;
    }
    lh += 1.0; // divisor
    lh += 3.0; // totais (subtotal + taxa + total)
    if order.coupon_code.as_deref().filter(|c| !c.is_empty()).is_some() {
        lh += 2.0;
    }
    lh += 1.4; // total
    // Observações como card com padding (label + texto + inner_pad 2x).
    // Estimativa generosa: 4 line_heights cobrem label + até 2 linhas
    // + padding interno (6mm ≈ 1.3 line_height).
    lh += 4.0;
    style.margin_mm * 2.0 + lh * style.line_height_mm + style.line_height_mm * 4.0
}

fn estimate_height_kitchen(order: &Order, phone: &str, style: &Style) -> f32 {
    let mut lh = 0.0_f32;
    lh += 2.0; // header
    lh += 1.2; // modalidade
    lh += 1.0; // divisor
    lh += 1.0; // cliente
    if !phone.is_empty() { lh += 1.0; } // telefone
    lh += 2.0; // endereço (caso seja entrega)
    lh += 1.0; // divisor
    lh += 1.2; // "ITENS"
    for it in &order.items {
        lh += 1.0;
        if it.addons_json.as_deref().filter(|s| !s.is_empty()).is_some() {
            lh += 1.7;
        }
        lh += 0.3;
    }
    lh += 4.0; // observações em card (label + texto + padding interno)
    style.margin_mm * 2.0 + lh * style.line_height_mm + style.line_height_mm * 4.0
}
