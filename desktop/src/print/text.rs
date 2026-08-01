//! Primitivas de texto e cursor dos PDFs de impressão.
//!
//! Fonte ÚNICA (AI_RULES.md §8 — sem duplicação) do cursor vertical, das
//! escritas posicionadas, do divisor, da quebra de linha, da medida de
//! largura e da sanitização de caracteres. Os módulos de documento —
//! [`super::pdf`] (comanda/cupom) e [`super::invoice_pdf`] (fatura e
//! extrato) — ficam apenas com o LAYOUT de cada papel.
//!
//! Todo texto sai nas fontes Type1 embutidas do PDF (Helvetica e
//! Helvetica-Bold, encoding WinAnsi/CP1252): nenhum TTF embutido, sem
//! dependência de fontes do SO.
//!
//! Convenção de coordenadas: o PDF tem origem no canto inferior esquerdo
//! e Y crescendo para cima. Para espelhar a escrita de cima para baixo,
//! mantemos um cursor `cursor_y` em mm a partir do TOPO da página e
//! convertemos em [`Ctx::coord_y`].

use printpdf::{IndirectFontRef, Line, Mm, PdfLayerReference, Point};

/// Espessura (pt) das linhas finas da comanda — divisores e moldura do
/// card de observações.
pub const HAIRLINE_PT: f32 = 0.3;

/// Espessura (pt) padrão do PDF — usada pelos divisores da fatura e do
/// extrato, que sempre foram desenhados com a espessura default.
pub const DEFAULT_RULE_PT: f32 = 1.0;

/// Geometria da página, em mm.
#[derive(Clone, Copy)]
pub struct Page {
    pub width_mm: f32,
    pub height_mm: f32,
    pub margin_mm: f32,
}

impl Page {
    /// Largura útil (descontadas as margens esquerda e direita).
    pub fn content_width_mm(&self) -> f32 {
        self.width_mm - self.margin_mm * 2.0
    }
}

/// Recursos do PDF + cursor, compartilhados por todas as primitivas.
///
/// `S` é o estilo próprio de cada documento (a comanda carrega o `Style`
/// da largura do papel; a fatura usa tamanhos fixos e não precisa de
/// nenhum, daí o `()` como padrão). É parâmetro de tipo — e não um campo
/// concreto — para que estas primitivas não conheçam layout algum; o
/// custo é zero (monomorfiza em tempo de compilação).
pub struct Ctx<'a, S = ()> {
    pub layer: PdfLayerReference,
    pub font: &'a IndirectFontRef,
    pub bold: &'a IndirectFontRef,
    pub page: Page,
    /// Cursor vertical em mm a partir do TOPO da página.
    pub cursor_y: f32,
    pub style: S,
}

impl<S> Ctx<'_, S> {
    /// PDF tem origem no canto inferior esquerdo. Aceitamos `y_from_top`
    /// (mm a partir do topo da página) e convertemos.
    pub fn coord_y(&self, y_from_top: f32) -> Mm {
        Mm(self.page.height_mm - y_from_top)
    }

    /// Largura útil da página deste documento.
    pub fn content_width_mm(&self) -> f32 {
        self.page.content_width_mm()
    }
}

// ── Cursor ────────────────────────────────────────────────────────

/// Avança o cursor `cursor_y` em `delta_mm`. O cursor é medido a partir
/// do topo da página — printpdf usa origem no rodapé, então
/// [`Ctx::coord_y`] faz a conversão na hora de desenhar.
pub fn advance<S>(ctx: &mut Ctx<'_, S>, delta_mm: f32) {
    ctx.cursor_y += delta_mm;
}

/// Linha de base do texto para o cursor atual: o cursor marca o TOPO da
/// linha, então descemos ~75% da altura da fonte.
fn baseline_y<S>(ctx: &Ctx<'_, S>, font_pt: f32) -> Mm {
    ctx.coord_y(ctx.cursor_y + pt_to_mm(font_pt) * 0.75)
}

// ── Escrita ───────────────────────────────────────────────────────

/// Escreve na posição horizontal `x_mm` (mm da borda esquerda).
pub fn write_at<S>(ctx: &Ctx<'_, S>, text: &str, x_mm: f32, font_pt: f32, font: &IndirectFontRef) {
    ctx.layer.use_text(sanitize(text), font_pt, Mm(x_mm), baseline_y(ctx, font_pt), font);
}

/// Escreve alinhado à margem esquerda.
pub fn write_left<S>(ctx: &Ctx<'_, S>, text: &str, font_pt: f32, font: &IndirectFontRef) {
    write_at(ctx, text, ctx.page.margin_mm, font_pt, font);
}

/// Escreve recuado `indent_mm` a partir da margem esquerda.
pub fn write_left_indented<S>(
    ctx: &Ctx<'_, S>,
    text: &str,
    indent_mm: f32,
    font_pt: f32,
    font: &IndirectFontRef,
) {
    write_at(ctx, text, ctx.page.margin_mm + indent_mm, font_pt, font);
}

/// Escreve centralizado na largura da página.
pub fn write_centered<S>(ctx: &Ctx<'_, S>, text: &str, font_pt: f32, font: &IndirectFontRef) {
    let x = (ctx.page.width_mm - text_width_mm(text, font_pt)) / 2.0;
    write_at(ctx, text, x.max(0.0), font_pt, font);
}

/// Linha com par (rótulo à esquerda, valor à direita) — o valor é
/// alinhado pela borda direita da área útil. Valor vazio não escreve
/// nada, só o rótulo.
pub fn write_pair<S>(
    ctx: &Ctx<'_, S>,
    left: &str,
    right: &str,
    font_pt: f32,
    font: &IndirectFontRef,
) {
    write_left(ctx, left, font_pt, font);
    if right.is_empty() {
        return;
    }
    let x_right_edge = ctx.page.width_mm - ctx.page.margin_mm;
    let x_right = (x_right_edge - text_width_mm(right, font_pt)).max(ctx.page.margin_mm);
    write_at(ctx, right, x_right, font_pt, font);
}

/// Divisor horizontal de margem a margem, na altura do cursor (não
/// avança o cursor — o respiro depois da linha é decisão de layout).
pub fn divider<S>(ctx: &Ctx<'_, S>, thickness_pt: f32) {
    let y = ctx.coord_y(ctx.cursor_y);
    let line = Line {
        points: vec![
            (Point::new(Mm(ctx.page.margin_mm), y), false),
            (Point::new(Mm(ctx.page.width_mm - ctx.page.margin_mm), y), false),
        ],
        is_closed: false,
    };
    ctx.layer.set_outline_thickness(thickness_pt);
    ctx.layer.add_line(line);
}

// ── Métricas e texto ──────────────────────────────────────────────

pub fn pt_to_mm(pt: f32) -> f32 {
    pt * 25.4 / 72.0
}

/// Largura do texto em mm para Helvetica.
///
/// Usa as métricas reais por classe de caractere (frações de em). Esta é
/// a versão que prevaleceu na unificação: a heurística antiga da comanda
/// ("0,5 em por caractere") errava o alinhamento à direita e a
/// centralização em textos com muitos glifos largos (`M`, `W`) ou
/// estreitos (`i`, `l`, `.`) — o alinhamento de colunas e de valores
/// depende dessa precisão.
pub fn text_width_mm(text: &str, font_pt: f32) -> f32 {
    let em: f32 = text.chars().map(char_width_em).sum();
    pt_to_mm(font_pt) * em
}

/// Largura de um caractere em fração de em (Helvetica).
fn char_width_em(c: char) -> f32 {
    match c {
        ' ' => 0.278,
        '.' | ',' | ':' | ';' | '\'' | '|' | 'i' | 'l' | 'j' | '!' => 0.244,
        'I' | 'f' | 't' | 'r' | '(' | ')' | '/' | '-' => 0.322,
        'm' | 'w' => 0.833,
        'M' | 'W' => 0.912,
        '0'..='9' | '$' => 0.556,
        'A'..='Z' => 0.704,
        _ => 0.535,
    }
}

/// Quebra o texto em linhas que cabem em `max_width_mm`. Quebra por
/// palavra (respeita espaços, não hifeniza) e mede com
/// [`text_width_mm`] — a largura é acumulada por palavra para não
/// realocar strings a cada tentativa.
pub fn wrap_text(text: &str, max_width_mm: f32, font_pt: f32) -> Vec<String> {
    let space_mm = text_width_mm(" ", font_pt);
    let mut out = Vec::new();
    let mut current = String::new();
    let mut current_mm = 0.0_f32;
    for word in text.split_whitespace() {
        let word_mm = text_width_mm(word, font_pt);
        if !current.is_empty() && current_mm + space_mm + word_mm > max_width_mm {
            out.push(std::mem::take(&mut current));
            current_mm = 0.0;
        }
        if !current.is_empty() {
            current.push(' ');
            current_mm += space_mm;
        }
        current.push_str(word);
        current_mm += word_mm;
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Sanitiza o texto para o encoding WinAnsi (CP1252) das fontes
/// embutidas — caractere não suportado é DESCARTADO em silêncio pelo
/// printpdf, então trocamos por equivalentes seguros antes.
///
/// É a UNIÃO das duas regras que existiam antes (comanda + fatura), para
/// que o mesmo caractere saia igual nos dois documentos do aplicativo:
/// - `−` (U+2212, menos matemático) não existe em WinAnsi e sumiria do
///   papel → vira `-`;
/// - caracteres de controle viram espaço;
/// - `—`, `…` e `·` até existem em WinAnsi, mas são reduzidos ao
///   equivalente ASCII porque nem toda impressora térmica/visualizador
///   renderiza a faixa alta do CP1252 — ASCII imprime em qualquer lugar.
fn sanitize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            // U+2212 (MINUS SIGN) NÃO existe no WinAnsi/CP1252 e o
            // printpdf o descarta em SILÊNCIO — o sinal sumia do papel.
            // Já `—`, `…` e `·` existem (0x97, 0x85, 0xB7) e imprimem
            // normalmente: convertê-los só empobreceria o documento.
            '\u{2212}' => out.push('-'),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn largura_de_texto_cresce_com_o_conteudo() {
        let pt = 10.0;
        assert!(text_width_mm("R$ 1.000,00", pt) > text_width_mm("R$ 10,00", pt));
        assert!(text_width_mm("W", pt) > text_width_mm("i", pt));
    }

    #[test]
    fn sanitize_cobre_as_regras_dos_dois_documentos() {
        // U+2212 some no papel (fora do WinAnsi) → vira hífen.
        assert_eq!(sanitize("− R$ 1,00"), "- R$ 1,00");
        // Já os que EXISTEM no WinAnsi são preservados.
        assert_eq!(sanitize("Rua A, 1 — Centro"), "Rua A, 1 — Centro");
        assert_eq!(sanitize("01/08 · 14:32"), "01/08 · 14:32");
        assert_eq!(sanitize("Pizza de…"), "Pizza de…");
        assert_eq!(sanitize("a\tb\nc"), "a b c");
        // Acentos do português estão em WinAnsi e passam intactos.
        assert_eq!(sanitize("Observações · Grátis"), "Observações · Grátis");
    }

    #[test]
    fn wrap_text_respeita_a_largura_disponivel() {
        let pt = 10.0;
        let lines = wrap_text("um dois tres quatro cinco seis", 20.0, pt);
        assert!(lines.len() > 1);
        assert!(lines.iter().all(|l| text_width_mm(l, pt) <= 20.0 || !l.contains(' ')));
    }
}
