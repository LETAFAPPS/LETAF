use slint::SharedString;

use letaf_core::error::CoreError;

use crate::MainWindow;

/// Exibe notificação toast no canto inferior direito.
///
/// Regras aplicadas (AI_RULES.md §8):
/// - Responsabilidade única: mostra mensagem visual
/// - Reseta visible para false/true para reiniciar o Timer Slint
pub(super) fn show_toast(ui: &MainWindow, message: &str, toast_type: &str) {
    ui.set_toast_visible(false);
    ui.set_toast_message(SharedString::from(message));
    ui.set_toast_type(SharedString::from(toast_type));
    ui.set_toast_visible(true);
}

/// Arco de uma fatia da MEIA-LUA (gauge semicircular superior) como
/// comando SVG. Viewbox 0 0 100 56, centro (50,50), raio 40. `start`/
/// `end` em fração (0..1) do semicírculo: 0 = ponta esquerda, 0.5 =
/// topo, 1 = ponta direita. Renderizado com stroke (espessura = anel).
/// Fatia vazia → string vazia. Compartilhado por Relatórios e Caixa.
pub(super) fn half_donut_arc(start: f64, end: f64) -> String {
    let sweep = end - start;
    if sweep <= 0.0001 {
        return String::new();
    }
    let (cx, cy, r) = (50.0_f64, 50.0_f64, 40.0_f64);
    // f=0 → 180° (esquerda); f=0.5 → 270° (topo); f=1 → 360° (direita).
    let point = |f: f64| {
        let a = (180.0 + f * 180.0).to_radians();
        (cx + r * a.cos(), cy + r * a.sin())
    };
    let (x0, y0) = point(start);
    let (x1, y1) = point(end);
    // Cada fatia ≤ 180° → large-arc-flag = 0; sweep-flag = 1 (horário,
    // varrendo por cima da esquerda para a direita).
    format!("M {:.3} {:.3} A {r} {r} 0 0 1 {:.3} {:.3}", x0, y0, x1, y1)
}

/// Converte um erro do core em mensagem pt-BR para o usuário, sem o
/// prefixo técnico do `Display` (ex.: "Validation:", "Not found:").
/// Validação já vem em pt-BR; os demais erros recebem texto genérico —
/// o erro cru deve ir só para o log (`tracing`), nunca pro toast.
pub(super) fn user_error(e: &CoreError) -> String {
    match e {
        CoreError::Validation(m) => m.clone(),
        CoreError::NotFound(_) => "Registro não encontrado.".to_string(),
        CoreError::Unauthorized(_) => "Acesso não autorizado.".to_string(),
        CoreError::Repository(_) => "Erro ao acessar os dados. Tente novamente.".to_string(),
    }
}
