//! Matemática do color picker das Configurações ("Cores do site"): converte
//! a posição das barras (matiz 0..1 + tonalidade 0..1) numa cor e no hex
//! salvo em `Company.color_palette`, e o caminho inverso (hex → posições)
//! para reposicionar as barras ao carregar. AI_RULES §8 (fonte única) / §11
//! (só apresentação; o backend valida o hex).

use slint::Color;

/// Saturação fixa das barras. A tonalidade controla o "valor" (brilho).
const SAT: f32 = 0.9;

/// tonalidade (0..1) → valor HSV (0.35..1.0): 0 = vívido, 1 = escuro.
fn tone_to_val(tone: f32) -> f32 {
    1.0 - tone.clamp(0.0, 1.0) * 0.65
}

/// valor HSV → tonalidade (inverso de `tone_to_val`).
fn val_to_tone(v: f32) -> f32 {
    ((1.0 - v) / 0.65).clamp(0.0, 1.0)
}

/// HSV (h em graus 0..360, s/v 0..1) → RGB 0..255.
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let h = h.rem_euclid(360.0) / 60.0;
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match h as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let to = |f: f32| ((f + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    (to(r), to(g), to(b))
}

/// RGB 0..255 → HSV (h graus, s, v).
fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / d).rem_euclid(6.0))
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let s = if max == 0.0 { 0.0 } else { d / max };
    (h.rem_euclid(360.0), s, max)
}

fn parse_hex(hex: &str) -> Option<(u8, u8, u8)> {
    let h = hex.trim().strip_prefix('#')?;
    let full = match h.len() {
        3 => h.chars().flat_map(|c| [c, c]).collect::<String>(),
        6 => h.to_string(),
        _ => return None,
    };
    let p = |i: usize| u8::from_str_radix(full.get(i..i + 2)?, 16).ok();
    Some((p(0)?, p(2)?, p(4)?))
}

/// Resultado do picker: prévia, extremos do gradiente de tonalidade e o hex.
pub struct Picked {
    pub preview: Color,
    pub tone_from: Color,
    pub tone_to: Color,
    pub hex: String,
}

/// A partir de matiz (0..1) + tonalidade (0..1) → cores + hex.
pub fn from_hue_tone(hue_frac: f32, tone: f32) -> Picked {
    let h = hue_frac.clamp(0.0, 1.0) * 360.0;
    let (r, g, b) = hsv_to_rgb(h, SAT, tone_to_val(tone));
    let (fr, fg, fb) = hsv_to_rgb(h, SAT, 1.0);
    let (tr, tg, tb) = hsv_to_rgb(h, SAT, 0.35);
    Picked {
        preview: Color::from_rgb_u8(r, g, b),
        tone_from: Color::from_rgb_u8(fr, fg, fb),
        tone_to: Color::from_rgb_u8(tr, tg, tb),
        hex: format!("#{r:02x}{g:02x}{b:02x}"),
    }
}

/// Do hex salvo → (matiz 0..1, tonalidade 0..1, cores). `None` se inválido.
pub fn from_hex(hex: &str) -> Option<(f32, f32, Picked)> {
    let (r, g, b) = parse_hex(hex)?;
    let (h, _s, v) = rgb_to_hsv(r, g, b);
    let hue_frac = h / 360.0;
    let tone = val_to_tone(v);
    let mut picked = from_hue_tone(hue_frac, tone);
    // Prévia com a cor REAL salva (não a recomputada), caso a saturação
    // divirja do padrão do picker.
    picked.preview = Color::from_rgb_u8(r, g, b);
    Some((hue_frac, tone, picked))
}
