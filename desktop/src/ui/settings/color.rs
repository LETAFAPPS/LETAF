//! Matemática do color picker das Configurações ("Cores do site"): converte
//! a posição das barras (matiz 0..1 + tonalidade 0..1) numa cor e no hex
//! salvo em `Company.color_palette`, e o caminho inverso (hex → posições)
//! para reposicionar as barras ao carregar. AI_RULES §8 (fonte única) / §11
//! (só apresentação; o backend valida o hex).

use slint::Color;

/// Saturação fixa das barras. A luminosidade (tonalidade) leva a cor de
/// BRANCO a PRETO passando pela cor viva.
const SAT: f32 = 0.9;

/// Amplitude do matiz na barra: 0..330° (em vez de 0..360°) para o espectro
/// NÃO repetir o vermelho no fim — a barra vai do vermelho (esq.) ao rosa
/// (dir.), cobrindo todas as cores uma única vez.
const HUE_SPAN: f32 = 330.0;

/// tonalidade (0..1) → luminosidade HSL (1.0..0.0): 0 = branco, 0.5 = cor
/// viva, 1 = preto.
fn tone_to_light(tone: f32) -> f32 {
    1.0 - tone.clamp(0.0, 1.0)
}

/// luminosidade HSL → tonalidade (inverso de `tone_to_light`).
fn light_to_tone(l: f32) -> f32 {
    (1.0 - l).clamp(0.0, 1.0)
}

/// HSL (h em graus 0..360, s/l 0..1) → RGB 0..255.
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h.rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let to = |f: f32| ((f + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    (to(r), to(g), to(b))
}

/// RGB 0..255 → HSL (h graus, s, l).
fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let l = (max + min) / 2.0;
    let s = if d == 0.0 {
        0.0
    } else {
        d / (1.0 - (2.0 * l - 1.0).abs())
    };
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / d).rem_euclid(6.0))
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h.rem_euclid(360.0), s, l)
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

/// Resultado do picker: prévia, a cor VIVA do matiz (meio do gradiente de
/// tonalidade — branco e preto são fixos nas pontas) e o hex.
pub struct Picked {
    pub preview: Color,
    pub tone_mid: Color,
    pub hex: String,
}

/// A partir de matiz (0..1) + tonalidade (0..1) → cores + hex.
pub fn from_hue_tone(hue_frac: f32, tone: f32) -> Picked {
    let h = hue_frac.clamp(0.0, 1.0) * HUE_SPAN;
    let (r, g, b) = hsl_to_rgb(h, SAT, tone_to_light(tone));
    let (mr, mg, mb) = hsl_to_rgb(h, SAT, 0.5);
    Picked {
        preview: Color::from_rgb_u8(r, g, b),
        tone_mid: Color::from_rgb_u8(mr, mg, mb),
        hex: format!("#{r:02x}{g:02x}{b:02x}"),
    }
}

/// Do hex salvo → (matiz 0..1, tonalidade 0..1, cores). `None` se inválido.
pub fn from_hex(hex: &str) -> Option<(f32, f32, Picked)> {
    let (r, g, b) = parse_hex(hex)?;
    let (h, _s, l) = rgb_to_hsl(r, g, b);
    let hue_frac = (h / HUE_SPAN).clamp(0.0, 1.0);
    let tone = light_to_tone(l);
    let mut picked = from_hue_tone(hue_frac, tone);
    // Prévia com a cor REAL salva (não a recomputada), caso a saturação
    // divirja do padrão do picker.
    picked.preview = Color::from_rgb_u8(r, g, b);
    Some((hue_frac, tone, picked))
}
