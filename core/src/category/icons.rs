//! Allowlist de ícones para `Category.icon_name`.
//!
//! Regras aplicadas (AI_RULES.md §1, §3, §8, §11):
//! - O slug do ícone é a fonte da verdade no banco — o SVG fica nas
//!   bibliotecas client (web/desktop), que mapeiam o slug para o
//!   markup local. Mantém o payload de sync pequeno e funciona
//!   offline.
//! - `is_valid` é usado pelo `CategoryService` para rejeitar slugs
//!   fora da lista (defesa de borda — frontend não é confiável).
//! - Adicionar um ícone é uma mudança de release: novo entry aqui +
//!   markup nos clientes. Categorias antigas com slugs removidos
//!   passam a renderizar como "sem ícone" (sem erro).
//!
//! Por que não armazenar a imagem em si: além do impacto de tamanho
//! no sync (offline-first), garante identidade visual consistente
//! — os clientes pintam o ícone com a cor do tema/estado, o que não
//! daria pra fazer com PNGs estáticos.

/// Tuplas `(slug, label_pt_br)` aceitas pelo backend e usadas pela
/// UI para popular o picker do formulário de categoria.
///
/// Convenções para o slug:
/// - kebab-case ASCII (`ice-cream`, não `Ice Cream`).
/// - Estável: nunca renomear um slug existente (vira "ícone órfão"
///   nas categorias salvas). Para depreciar, manter o slug e parar
///   de oferecer no picker do client.
pub const ICONS: &[(&str, &str)] = &[
    ("ice-cream",   "Sorvete"),
    ("drink",       "Bebida"),
    ("pizza",       "Pizza"),
    ("burger",      "Lanche"),
    ("combo",       "Combo"),
    ("snack",       "Salgado"),
    ("dessert",     "Sobremesa"),
    ("candy",       "Doce"),
    ("coffee",      "Café"),
    ("bread",       "Pão"),
    ("salad",       "Espetinho"),
    ("meat",        "Carne"),
    ("convenience", "Conveniência"),
];

/// `true` se o slug está na allowlist. Usado pelo service em
/// create/update para rejeitar entrada inválida vinda do client.
pub fn is_valid(slug: &str) -> bool {
    ICONS.iter().any(|(s, _)| *s == slug)
}
