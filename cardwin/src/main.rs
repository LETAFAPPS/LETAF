//! `letaf-cardwin` — janela de cadastro de cartão (WebView) do LETAF.
//!
//! Abre a página hospedada da Efi.js (`.../pay/card?s=<sessão>`) DENTRO de
//! uma janela do próprio app, em vez do navegador externo. Roda em PROCESSO
//! separado de propósito: o desktop usa o event-loop do Slint (winit) e o
//! WebKitGTK precisa do seu próprio loop/GTK na main thread — isolar em outro
//! processo evita o conflito.
//!
//! Segurança (AI_RULES §11): a tokenização do cartão é client-side (Efi.js);
//! número/CVV NUNCA passam pelo nosso servidor. Esta janela só exibe a página.
//!
//! Uso: `letaf-cardwin <url>`.

use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop};
use tao::window::WindowBuilder;
use wry::WebViewBuilder;

fn main() -> wry::Result<()> {
    // WebKitGTK renderiza EM BRANCO em vários ambientes (Wayland,
    // virtualizado, certos drivers) quando usa o renderer DMABUF/compositing.
    // Desligar esses caminhos força a renderização por software e resolve a
    // janela preta. Precisa vir ANTES de inicializar o GTK/WebView.
    std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");

    let url = std::env::args().nth(1).unwrap_or_default();
    if url.is_empty() {
        eprintln!("uso: letaf-cardwin <url>");
        std::process::exit(2);
    }

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("LETAF · Cadastro de Cartão")
        .with_inner_size(LogicalSize::new(480.0, 760.0))
        .with_resizable(true)
        .build(&event_loop)
        .expect("falha ao criar a janela");

    let _webview = WebViewBuilder::new().with_url(&url).build(&window)?;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            *control_flow = ControlFlow::Exit;
        }
    });
}
