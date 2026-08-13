//! Paletas de cores do site (cardápio web), escolhidas por CADA empresa nas
//! Configurações. Fonte ÚNICA: o servidor resolve o slug salvo na empresa
//! (`Company.color_palette`) para as 5 cores e o web as aplica inline. O
//! desktop mostra os swatches para escolher o slug.
//!
//! Diferença para o TEMA DO TIPO (`business_type.theme`): o tipo dá o visual
//! PADRÃO por ramo; a paleta da empresa SOBREPÕE as cores quando escolhida.
//! Slugs são um conjunto FIXO — paletas coerentes, sem risco de contraste ruim.

/// Uma paleta = as 5 variáveis de cor do site (mesmas do `main.scss`).
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub slug: &'static str,
    pub label: &'static str,
    pub brand: &'static str,
    pub price: &'static str,
    pub ink: &'static str,
    pub muted: &'static str,
    pub line: &'static str,
}

/// Catálogo fixo de paletas oferecidas à empresa (swatch por `brand`).
pub const PALETTES: &[Palette] = &[
    Palette { slug: "laranja",  label: "Laranja",  brand: "#e8731c", price: "#66bb6a", ink: "#1a1a1a", muted: "#9ca3af", line: "#e5e7eb" },
    Palette { slug: "azul",     label: "Azul",     brand: "#2563eb", price: "#16a34a", ink: "#111827", muted: "#94a3b8", line: "#e5e7eb" },
    Palette { slug: "verde",    label: "Verde",    brand: "#16a34a", price: "#15803d", ink: "#14211a", muted: "#94a3b8", line: "#e5e7eb" },
    Palette { slug: "vermelho", label: "Vermelho", brand: "#dc2626", price: "#16a34a", ink: "#1a1414", muted: "#9ca3af", line: "#e5e7eb" },
    Palette { slug: "roxo",     label: "Roxo",     brand: "#7c3aed", price: "#16a34a", ink: "#1a1420", muted: "#9ca3af", line: "#e7e5ea" },
    Palette { slug: "teal",     label: "Teal",     brand: "#0e7490", price: "#16a34a", ink: "#0f172a", muted: "#94a3b8", line: "#e2e8f0" },
    Palette { slug: "rosa",     label: "Rosa",     brand: "#db2777", price: "#16a34a", ink: "#1f1418", muted: "#9ca3af", line: "#efe5ea" },
    Palette { slug: "grafite",  label: "Grafite",  brand: "#334155", price: "#16a34a", ink: "#0f172a", muted: "#94a3b8", line: "#e2e8f0" },
];

/// Resolve o slug para a paleta correspondente (`None` se desconhecido/vazio).
pub fn palette_by_slug(slug: &str) -> Option<&'static Palette> {
    let s = slug.trim();
    if s.is_empty() {
        return None;
    }
    PALETTES.iter().find(|p| p.slug == s)
}

/// `true` se `slug` é uma paleta válida do catálogo.
pub fn palette_is_valid(slug: &str) -> bool {
    palette_by_slug(slug).is_some()
}

/// `true` se `s` é uma cor de marca LIVRE em hex (`#RGB` ou `#RRGGBB`).
/// A empresa escolhe a cor no color picker do desktop; guardamos o hex em
/// `Company.color_palette`. Defesa de borda (§11): o backend valida antes
/// de persistir/servir.
pub fn is_brand_hex(s: &str) -> bool {
    let h = s.trim();
    match h.strip_prefix('#') {
        Some(rest) => (rest.len() == 3 || rest.len() == 6) && rest.chars().all(|c| c.is_ascii_hexdigit()),
        None => false,
    }
}
