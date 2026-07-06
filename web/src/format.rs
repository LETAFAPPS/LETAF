//! Helpers de formatação para a apresentação (puros).

/// Valor monetário (mesmo formato do app: `R$ 12.34`).
pub fn money(v: f64) -> String {
    format!("R$ {v:.2}")
}

/// Quantidade: inteiro quando exato (ex.: `2`), senão até 3 casas
/// (produtos por peso, ex.: `0.350`).
pub fn qty(q: f64) -> String {
    if q.fract().abs() < 0.001 {
        format!("{q:.0}")
    } else {
        format!("{q:.3}")
    }
}

/// Extrai a mensagem amigável do fim de um erro técnico (ex.: o prefixo
/// do `ServerFnError`), para exibir ao usuário.
pub fn server_error(raw: &str) -> String {
    raw.rsplit(": ").next().unwrap_or(raw).trim().to_string()
}

/// Monta o `src` da imagem a partir do `image_data` da API: se já for um
/// data URL, usa direto; senão, prefixa o MIME inferido da assinatura
/// base64 (PNG/WEBP/JPEG).
pub fn image_data_url(image_data: &str) -> String {
    if image_data.starts_with("data:") {
        return image_data.to_string();
    }
    let mime = if image_data.starts_with("iVBOR") {
        "image/png"
    } else if image_data.starts_with("UklGR") {
        "image/webp"
    } else {
        "image/jpeg"
    };
    format!("data:{mime};base64,{image_data}")
}
