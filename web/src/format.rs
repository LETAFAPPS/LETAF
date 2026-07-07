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
