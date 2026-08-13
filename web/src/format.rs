//! Helpers de formatação para a apresentação (puros).

/// Valor monetário em pt-BR: vírgula decimal e ponto de milhar
/// (ex.: `R$ 1.234,50`). O preço é o dado mais lido do cardápio — ponto
/// decimal ("R$ 12.34") destoa do padrão brasileiro.
pub fn money(v: f64) -> String {
    let neg = v < 0.0;
    let cents = (v.abs() * 100.0).round() as u64;
    let reais = (cents / 100).to_string();
    let dec = cents % 100;
    // Agrupa o inteiro em milhares com ponto.
    let bytes = reais.as_bytes();
    let mut inteiro = String::with_capacity(reais.len() + reais.len() / 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i).is_multiple_of(3) {
            inteiro.push('.');
        }
        inteiro.push(*b as char);
    }
    format!("{}R$ {inteiro},{dec:02}", if neg { "-" } else { "" })
}

/// Quantidade: inteiro quando exato (ex.: `2`), senão até 3 casas com
/// vírgula (produtos por peso, ex.: `0,350`).
pub fn qty(q: f64) -> String {
    if q.fract().abs() < 0.001 {
        format!("{q:.0}")
    } else {
        format!("{q:.3}").replace('.', ",")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn moeda_pt_br() {
        assert_eq!(money(12.34), "R$ 12,34");
        assert_eq!(money(1234.5), "R$ 1.234,50");
        assert_eq!(money(0.0), "R$ 0,00");
        assert_eq!(money(1_234_567.89), "R$ 1.234.567,89");
        assert_eq!(money(-5.0), "-R$ 5,00");
    }
    #[test]
    fn quantidade_virgula() {
        assert_eq!(qty(2.0), "2");
        assert_eq!(qty(0.35), "0,350");
    }
}

/// Data ISO ("2026-08-12T10:30:00") → "12/08/2026" (pt-BR). Se o formato
/// não casar, devolve os 10 primeiros caracteres como estavam.
pub fn iso_date_br(iso: &str) -> String {
    let d = iso.get(..10).unwrap_or(iso);
    match d.split('-').collect::<Vec<_>>().as_slice() {
        [y, m, dd] => format!("{dd}/{m}/{y}"),
        _ => d.to_string(),
    }
}

/// Extrai a mensagem amigável do fim de um erro técnico (ex.: o prefixo
/// do `ServerFnError`), para exibir ao usuário.
pub fn server_error(raw: &str) -> String {
    raw.rsplit(": ").next().unwrap_or(raw).trim().to_string()
}
