//! Geração e envio de PDFs de impressão (comandas, NFC-e no futuro).
//!
//! Responsabilidades (AI_RULES.md §8):
//! - `pdf.rs` — monta o documento PDF a partir do `Order` + dados
//!   resolvidos (nome/telefone formatados, largura do papel).
//! - Quem chama (em `ui::orders::setup_print_receipt_now`) salva os
//!   bytes em arquivo temporário e dispara o spooler do SO.
//!
//! O layout é desenhado para casar visualmente com o `ReceiptModal`
//! Slint — fontes proporcionais, divisórias reais e alinhamento de
//! pares (label/valor) — não com texto monospace.

pub mod pdf;
