//! Callbacks da carteira do cliente (Fase 12).
//!
//! AI_RULES.md §1, §3, §8, §11, §14: UI dispara callbacks; Rust faz I/O,
//! validação e formatação; toda derivação acontece aqui.
//!
//! Dividido por responsabilidade:
//! - `core`: orquestrador (`setup_wallet`), refresh e agregação (saldo/movimentos)
//! - `view`: aplicação na UI, listeners de seleção e abertura/fechamento de modais
//! - `ops`: confirmação de depósito/saque/ajuste/limite e sync listener

mod core;
mod ops;
mod view;

pub(crate) use core::setup_wallet;
