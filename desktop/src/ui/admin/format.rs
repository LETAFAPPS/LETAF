//! Formatação/parse dos valores exibidos no painel (funções puras).

/// Formata um valor em reais ("R$ 2.000,00"). Delega ao helper canônico
/// (a versão anterior não tinha separador de milhar — AI_RULES §8).
pub(super) fn brl(v: f64) -> String {
    crate::format::money_br_f64(v)
}

/// Converte um valor monetário digitado (pt-BR ou simples) em `f64`.
/// Aceita "30", "30,00", "1.234,56", "R$ 30". Inválido → 0.
pub(super) fn parse_money_br(raw: &str) -> f64 {
    let cleaned = raw.trim().replace("R$", "").replace(' ', "");
    let normalized = if cleaned.contains(',') {
        cleaned.replace('.', "").replace(',', ".")
    } else {
        cleaned
    };
    normalized.parse::<f64>().unwrap_or(0.0).max(0.0)
}

/// Benefícios do plano a partir da descrição livre: uma linha por item.
/// Aceita quebra de linha, "·", ";" ou "•" como separador — o admin
/// escreve como preferir e o card lista com "✓".
pub(super) fn plan_features(description: &str) -> Vec<slint::SharedString> {
    let normalized = description
        .replace(['·', ';', '•'], "\n")
        .replace(" - ", "\n");
    normalized
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .take(6)
        .map(slint::SharedString::from)
        .collect()
}
