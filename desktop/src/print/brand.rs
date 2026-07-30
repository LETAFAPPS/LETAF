//! Marca LETAF (estrela + wordmark) para os PDFs.
//!
//! As artes originais são BRANCAS com transparência (feitas para o
//! fundo escuro do app). Aqui elas são recompostas sobre branco na cor
//! do texto — o PDF tem fundo claro. A composição preserva o
//! anti-aliasing (usa o alpha como fator de mistura) e devolve RGB8
//! puro, sem canal alpha: `ImageXObject` sem `smask`, que é o formato
//! que o `printpdf` grava direto.

use printpdf::{ColorBits, ColorSpace, Image, ImageTransform, ImageXObject, Mm, Px};

/// Estrela do LETAF (ícone quadrado).
const STAR_PNG: &[u8] = include_bytes!("../../ui/assets/brand/icone.png");
/// Wordmark "LETAF".
const WORDMARK_PNG: &[u8] = include_bytes!("../../ui/assets/brand/letaf-wordmark.png");

/// Cor com que a marca é impressa (mesmo tom do texto principal).
const INK: [u8; 3] = [0x1C, 0x1A, 0x18];

/// Marca decodificada e pronta para o PDF.
pub struct BrandImage {
    image: Image,
    width_px: u32,
    height_px: u32,
}

impl BrandImage {
    /// Altura em mm dado o alvo de largura, preservando a proporção.
    pub fn height_for_width(&self, width_mm: f32) -> f32 {
        width_mm * self.height_px as f32 / self.width_px as f32
    }

    /// Desenha com `width_mm` de largura; `y_from_top` é a distância do
    /// TOPO da página até o TOPO da imagem.
    pub fn draw(
        self,
        layer: &printpdf::PdfLayerReference,
        x_mm: f32,
        y_from_top_mm: f32,
        width_mm: f32,
        page_height_mm: f32,
    ) {
        let height_mm = self.height_for_width(width_mm);
        // O `dpi` é o que define a escala física: largura_mm =
        // width_px * 25.4 / dpi.
        let dpi = self.width_px as f32 * 25.4 / width_mm;
        let baseline = page_height_mm - y_from_top_mm - height_mm;
        self.image.add_to_layer(
            layer.clone(),
            ImageTransform {
                translate_x: Some(Mm(x_mm)),
                translate_y: Some(Mm(baseline)),
                dpi: Some(dpi),
                ..Default::default()
            },
        );
    }
}

/// Estrela do LETAF na cor do texto (`None` se a arte não decodificar).
pub fn star(target_px: u32) -> Option<BrandImage> {
    load(STAR_PNG, target_px)
}

/// Wordmark "LETAF" na cor do texto.
pub fn wordmark(target_px: u32) -> Option<BrandImage> {
    load(WORDMARK_PNG, target_px)
}

/// Decodifica, recorta o excesso transparente, redimensiona para
/// `target_px` de largura e compõe sobre branco na cor da tinta.
fn load(bytes: &[u8], target_px: u32) -> Option<BrandImage> {
    use image::GenericImageView;
    let img = image::load_from_memory(bytes).ok()?;
    let img = crop_opaque(&img);
    let (w, _) = img.dimensions();
    let img = if w > target_px {
        let ratio = target_px as f32 / w as f32;
        let new_h = ((img.height() as f32 * ratio).round() as u32).max(1);
        img.resize_exact(target_px, new_h, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    let (width_px, height_px) = rgba.dimensions();
    let mut data = Vec::with_capacity((width_px * height_px * 3) as usize);
    for px in rgba.pixels() {
        let a = px[3] as f32 / 255.0;
        for (i, ink) in INK.iter().enumerate() {
            let _ = i;
            // branco quando transparente; tinta quando opaco.
            let v = 255.0 * (1.0 - a) + *ink as f32 * a;
            data.push(v.round().clamp(0.0, 255.0) as u8);
        }
    }
    let xobject = ImageXObject {
        width: Px(width_px as usize),
        height: Px(height_px as usize),
        color_space: ColorSpace::Rgb,
        bits_per_component: ColorBits::Bit8,
        interpolate: true,
        image_data: data,
        image_filter: None,
        smask: None,
        clipping_bbox: None,
    };
    Some(BrandImage {
        image: Image::from(xobject),
        width_px,
        height_px,
    })
}

/// Recorta as bordas totalmente transparentes (as artes vêm com
/// margem vazia, que desalinharia a marca no cabeçalho).
fn crop_opaque(img: &image::DynamicImage) -> image::DynamicImage {
    use image::GenericImageView;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0u32, 0u32);
    for (x, y, px) in rgba.enumerate_pixels() {
        if px[3] > 8 {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    if x1 <= x0 || y1 <= y0 {
        return img.clone();
    }
    img.view(x0, y0, x1 - x0 + 1, y1 - y0 + 1).to_image().into()
}
