use std::path::{Path, PathBuf};

/// Decodifica base64 → pixels RGBA brutos (SharedPixelBuffer — Send).
///
/// Regras aplicadas (AI_RULES.md §8):
/// - Função com responsabilidade única: apenas decode de bytes
/// - Retorna None em caso de erro (sem panic)
pub(crate) fn decode_pixel_buffer(b64: &str) -> Option<slint::SharedPixelBuffer<slint::Rgba8Pixel>> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    let img = image::load_from_memory(&bytes).ok()?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    Some(slint::SharedPixelBuffer::clone_from_slice(rgba.as_raw(), w, h))
}

/// Decodifica a imagem de um único produto em thread pool.
///
/// Regras aplicadas (AI_RULES.md §8, §13):
/// - Responsabilidade única: decode de 1 imagem
/// - Evita re-decode de N imagens após cada operação
pub(super) async fn decode_single_product_image(
    image_data: Option<String>,
) -> Option<slint::SharedPixelBuffer<slint::Rgba8Pixel>> {
    let b64 = image_data.filter(|s| !s.is_empty())?;
    tokio::task::spawn_blocking(move || decode_pixel_buffer(&b64))
        .await
        .unwrap_or(None)
}

/// Abre o seletor de arquivo de imagem nativo.
///
/// Regras aplicadas (AI_RULES.md §8): responsabilidade única — apenas UI de seleção.
pub(super) fn pick_image_file() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Imagem", &["png", "jpg", "jpeg", "webp"])
        .pick_file()
}

/// Lê o arquivo, redimensiona e retorna a imagem como JPEG base64.
///
/// Regras aplicadas (AI_RULES.md §8): responsabilidade única — I/O + encode.
pub(super) fn process_image_file(path: &Path) -> Option<String> {
    use base64::Engine;
    let bytes = std::fs::read(path).ok()?;
    let jpeg_bytes = resize_to_jpeg(&bytes, 400, 82)?;
    Some(base64::engine::general_purpose::STANDARD.encode(&jpeg_bytes))
}

/// Versão específica para produtos: além do binário base64, executa a
/// heurística `detect_cover_color` sobre a imagem original (com canal
/// alpha) para classificar o fundo. Veja `detect_cover_color` para a
/// regra de classificação.
///
/// Regras aplicadas (AI_RULES.md §8, §11):
/// - Heurística roda ANTES da conversão (que pode descartar alpha).
/// - O formato de saída preserva transparência quando ela existe:
///   - Imagem com pixels alpha < 255 → PNG (mantém transparência).
///   - Imagem totalmente opaca → JPEG (footprint menor).
///
/// Isso evita o bug em que um PNG transparente convertido para JPEG
/// ficava com fundo preto "queimado" na imagem.
pub(super) fn process_product_image(path: &Path) -> Option<(String, Option<String>)> {
    use base64::Engine;
    let bytes = std::fs::read(path).ok()?;
    let img = image::load_from_memory(&bytes).ok()?;
    let cover = detect_cover_color(&img);
    let encoded = resize_decoded_for_storage(img, 400, 82)?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&encoded);
    Some((b64, cover))
}

/// Lê o arquivo, redimensiona para capa (1200px) e retorna JPEG base64.
pub(super) fn process_image_file_large(path: &Path) -> Option<String> {
    use base64::Engine;
    let bytes = std::fs::read(path).ok()?;
    let jpeg_bytes = resize_to_jpeg(&bytes, 1200, 85)?;
    Some(base64::engine::general_purpose::STANDARD.encode(&jpeg_bytes))
}

/// Decodifica qualquer formato (WebP/PNG/JPEG), redimensiona e re-encoda como JPEG.
///
/// Regras aplicadas (AI_RULES.md §8, §13):
/// - Responsabilidade única: transformação de imagem
/// - Normaliza para JPEG garantindo MIME type correto na web e decode uniforme no desktop
pub(super) fn resize_to_jpeg(bytes: &[u8], max_px: u32, quality: u8) -> Option<Vec<u8>> {
    let img = image::load_from_memory(bytes).ok()?;
    resize_decoded_to_jpeg(img, max_px, quality)
}

/// Redimensiona uma imagem já decodificada e a encoda como JPEG.
fn resize_decoded_to_jpeg(img: image::DynamicImage, max_px: u32, quality: u8) -> Option<Vec<u8>> {
    use image::codecs::jpeg::JpegEncoder;
    use std::io::Cursor;
    let img = if img.width() > max_px || img.height() > max_px {
        img.thumbnail(max_px, max_px)
    } else {
        img
    };
    // JPEG não suporta alpha — converte para RGB para evitar artefatos.
    let img = image::DynamicImage::ImageRgb8(img.to_rgb8());
    let mut buf = Vec::new();
    let encoder = JpegEncoder::new_with_quality(Cursor::new(&mut buf), quality);
    img.write_with_encoder(encoder).ok()?;
    Some(buf)
}

/// Redimensiona e escolhe o formato de saída preservando transparência
/// quando ela existe.
///
/// Regras aplicadas (AI_RULES.md §8, §13):
/// - Imagens com pixels realmente transparentes → PNG (preserva alpha).
/// - Imagens totalmente opacas → JPEG (≈10× menor).
/// - Sem este branch, PNGs transparentes vinham sendo convertidos para
///   JPEG e o alpha-compositing do crate `image` substituía o fundo por
///   preto, contaminando a imagem na grade.
fn resize_decoded_for_storage(
    img: image::DynamicImage,
    max_px: u32,
    quality: u8,
) -> Option<Vec<u8>> {
    use image::codecs::jpeg::JpegEncoder;
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    use std::io::Cursor;
    let img = if img.width() > max_px || img.height() > max_px {
        img.thumbnail(max_px, max_px)
    } else {
        img
    };
    if image_has_real_transparency(&img) {
        let rgba = image::DynamicImage::ImageRgba8(img.to_rgba8());
        let mut buf = Vec::new();
        let encoder = PngEncoder::new_with_quality(
            Cursor::new(&mut buf),
            CompressionType::Default,
            FilterType::Adaptive,
        );
        rgba.write_with_encoder(encoder).ok()?;
        Some(buf)
    } else {
        let rgb = image::DynamicImage::ImageRgb8(img.to_rgb8());
        let mut buf = Vec::new();
        let encoder = JpegEncoder::new_with_quality(Cursor::new(&mut buf), quality);
        rgb.write_with_encoder(encoder).ok()?;
        Some(buf)
    }
}

/// `true` se a imagem tem ao menos um pixel com alpha < 255 (amostra
/// 10×10 distribuída). Imagens em formatos sem alpha retornam `false`
/// imediatamente — sem custo de iteração.
fn image_has_real_transparency(img: &image::DynamicImage) -> bool {
    use image::GenericImageView;
    if !img.color().has_alpha() {
        return false;
    }
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 { return false; }
    for j in 0..10u32 {
        let y = (j * h / 10).min(h - 1);
        for i in 0..10u32 {
            let x = (i * w / 10).min(w - 1);
            if img.get_pixel(x, y)[3] < 255 {
                return true;
            }
        }
    }
    false
}

/// Heurística para classificar o fundo da imagem do produto.
///
/// Amostra os 4 cantos da imagem (com padding de 2 px) e decide:
/// - Todos com `alpha < 50`: imagem transparente → `None`.
///   O card cai na cor do tema (igual ao placeholder), permitindo que
///   produtos PNG sem fundo apareçam visualmente integrados.
/// - Todos opacos (`alpha > 200`) e RGB com delta máximo ≤ 20 entre
///   cantos: fundo uniforme → retorna a cor média em `#RRGGBB`. O card
///   pinta com essa cor, eliminando a "costura" entre imagem e card.
/// - Demais casos (sombras, gradiente, foto complexa): `None`.
///
/// Regras aplicadas (AI_RULES.md §8, §11):
/// - Função pura sobre pixels (sem I/O, sem dependências externas).
/// - Falhas (imagem minúscula, etc.) retornam `None` em vez de panic.
pub(super) fn detect_cover_color(img: &image::DynamicImage) -> Option<String> {
    use image::GenericImageView;
    let (w, h) = img.dimensions();
    if w < 8 || h < 8 { return None; }

    let pad = 2;
    let pts = [
        (pad, pad),
        (w - 1 - pad, pad),
        (pad, h - 1 - pad),
        (w - 1 - pad, h - 1 - pad),
    ];
    let corners: [image::Rgba<u8>; 4] = [
        img.get_pixel(pts[0].0, pts[0].1),
        img.get_pixel(pts[1].0, pts[1].1),
        img.get_pixel(pts[2].0, pts[2].1),
        img.get_pixel(pts[3].0, pts[3].1),
    ];

    if corners.iter().all(|p| p[3] < 50) {
        return None;
    }
    if !corners.iter().all(|p| p[3] > 200) {
        return None;
    }

    let avg_r = corners.iter().map(|p| p[0] as u32).sum::<u32>() / 4;
    let avg_g = corners.iter().map(|p| p[1] as u32).sum::<u32>() / 4;
    let avg_b = corners.iter().map(|p| p[2] as u32).sum::<u32>() / 4;

    let max_delta = corners
        .iter()
        .map(|p| {
            let dr = (p[0] as i32 - avg_r as i32).abs();
            let dg = (p[1] as i32 - avg_g as i32).abs();
            let db = (p[2] as i32 - avg_b as i32).abs();
            dr.max(dg).max(db)
        })
        .max()
        .unwrap_or(0);

    if max_delta <= 20 {
        Some(format!("#{:02X}{:02X}{:02X}", avg_r, avg_g, avg_b))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, Rgba, RgbaImage};

    fn solid(color: Rgba<u8>) -> DynamicImage {
        let img = RgbaImage::from_pixel(20, 20, color);
        DynamicImage::ImageRgba8(img)
    }

    #[test]
    fn detects_solid_white() {
        let img = solid(Rgba([255, 255, 255, 255]));
        assert_eq!(detect_cover_color(&img).as_deref(), Some("#FFFFFF"));
    }

    #[test]
    fn detects_transparent_as_none() {
        let img = solid(Rgba([0, 0, 0, 0]));
        assert_eq!(detect_cover_color(&img), None);
    }

    #[test]
    fn detects_solid_red() {
        let img = solid(Rgba([200, 30, 30, 255]));
        assert_eq!(detect_cover_color(&img).as_deref(), Some("#C81E1E"));
    }

    #[test]
    fn rejects_non_uniform_corners() {
        let mut img = RgbaImage::new(20, 20);
        img.put_pixel(2, 2, Rgba([255, 255, 255, 255]));
        img.put_pixel(17, 2, Rgba([0, 0, 0, 255]));
        img.put_pixel(2, 17, Rgba([255, 0, 0, 255]));
        img.put_pixel(17, 17, Rgba([0, 255, 0, 255]));
        assert_eq!(detect_cover_color(&DynamicImage::ImageRgba8(img)), None);
    }

    #[test]
    fn rejects_image_too_small() {
        let img = solid(Rgba([255, 255, 255, 255]));
        let small = img.crop_imm(0, 0, 4, 4);
        assert_eq!(detect_cover_color(&small), None);
    }

    #[test]
    fn transparent_corners_round_trip_as_png() {
        // PNG totalmente transparente deve voltar como PNG (bytes
        // começam com a assinatura PNG \x89PNG\r\n\x1a\n).
        let img = solid(Rgba([0, 0, 0, 0]));
        let bytes = resize_decoded_for_storage(img, 200, 80).expect("encoded");
        assert_eq!(&bytes[0..4], &[0x89, 0x50, 0x4E, 0x47]);
    }

    #[test]
    fn opaque_round_trips_as_jpeg() {
        // Imagem opaca deve voltar como JPEG (assinatura \xff\xd8\xff).
        let img = solid(Rgba([255, 255, 255, 255]));
        let bytes = resize_decoded_for_storage(img, 200, 80).expect("encoded");
        assert_eq!(&bytes[0..3], &[0xFF, 0xD8, 0xFF]);
    }

    #[test]
    fn semitransparent_preserves_alpha_as_png() {
        // Alpha parcial também conta como transparente real.
        let img = solid(Rgba([200, 100, 50, 128]));
        let bytes = resize_decoded_for_storage(img, 200, 80).expect("encoded");
        assert_eq!(&bytes[0..4], &[0x89, 0x50, 0x4E, 0x47]);
    }
}
