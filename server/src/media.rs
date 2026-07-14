//! Decodificação de imagens do catálogo para servir como bytes crus.
//!
//! As imagens são guardadas no banco como base64 (com ou sem prefixo
//! `data:<mime>;base64,`). Os endpoints públicos `/catalog/media/*` decodificam
//! sob demanda e respondem os bytes com `Content-Type` correto + cache longo,
//! deixando o HTML do SSR enxuto (AI_RULES §3, §13 — SEO/LCP). Este módulo
//! isola a parte pura (decode + detecção de MIME), testável sem HTTP.

use base64::{engine::general_purpose::STANDARD, Engine as _};

/// Cache "para sempre" — a URL carrega `?v=<updated_at>`, então muda quando a
/// imagem muda; o conteúdo em si é imutável para uma dada versão.
pub const IMMUTABLE_CACHE: &str = "public, max-age=31536000, immutable";

/// Decodifica o `image_data` (base64, com ou sem prefixo `data:`) em
/// `(bytes, content_type)`. `None` se vazio ou base64 inválido.
pub fn decode_image(data: &str) -> Option<(Vec<u8>, &'static str)> {
    let data = data.trim();
    if data.is_empty() {
        return None;
    }
    let (mime, payload) = match data.strip_prefix("data:") {
        Some(rest) => {
            let (meta, b64) = rest.split_once(',')?;
            (mime_from_meta(meta), b64)
        }
        None => (sniff_mime_b64(data), data),
    };
    let bytes = STANDARD.decode(payload).ok()?;
    if bytes.is_empty() {
        return None;
    }
    Some((bytes, mime))
}

/// MIME a partir do metadado de uma data URL (ex.: `image/png;base64`).
fn mime_from_meta(meta: &str) -> &'static str {
    match meta.split(';').next().unwrap_or("") {
        "image/png" => "image/png",
        "image/webp" => "image/webp",
        "image/gif" => "image/gif",
        // NÃO servir `image/svg+xml`: SVG na origem da loja executa JS
        // (XSS armazenado). As imagens do app são sempre raster (o processador
        // gera JPEG/PNG); um SVG só chega por payload forjado — cai no default
        // e é tratado como JPEG (render quebrado, sem execução). §11.
        _ => "image/jpeg",
    }
}

/// Detecta o MIME pelos primeiros caracteres do base64 cru (mesma heurística do
/// `web::format::image_data_url`): `iVBOR`→PNG, `UklGR`→WebP, `R0lGOD`→GIF,
/// senão assume JPEG.
fn sniff_mime_b64(b64: &str) -> &'static str {
    if b64.starts_with("iVBOR") {
        "image/png"
    } else if b64.starts_with("UklGR") {
        "image/webp"
    } else if b64.starts_with("R0lGOD") {
        "image/gif"
    } else {
        "image/jpeg"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // PNG mínimo (8 bytes de assinatura) em base64.
    const PNG_B64: &str = "iVBORw0KGgo=";

    #[test]
    fn decode_base64_cru_detecta_png() {
        let (bytes, mime) = decode_image(PNG_B64).unwrap();
        assert_eq!(mime, "image/png");
        assert_eq!(&bytes[..4], &[0x89, 0x50, 0x4E, 0x47]); // \x89PNG
    }

    #[test]
    fn decode_data_url_usa_mime_do_prefixo() {
        let data = format!("data:image/webp;base64,{PNG_B64}");
        let (_, mime) = decode_image(&data).unwrap();
        assert_eq!(mime, "image/webp"); // prefixo tem prioridade
    }

    #[test]
    fn vazio_ou_invalido_retorna_none() {
        assert!(decode_image("").is_none());
        assert!(decode_image("   ").is_none());
        assert!(decode_image("não é base64 @@@").is_none());
    }

    #[test]
    fn jpeg_e_o_fallback() {
        // "/9j/" é o começo típico de JPEG em base64.
        let (_, mime) = decode_image("/9j/4AAQSkZJRg==").unwrap();
        assert_eq!(mime, "image/jpeg");
    }
}
