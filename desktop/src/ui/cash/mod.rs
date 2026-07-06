//! Callbacks da gestão de caixa.
//!
//! AI_RULES.md §1, §3, §8, §11, §14: UI dispara callbacks; Rust faz I/O e
//! formatação; o service centraliza as regras. Valores/datas/durações já
//! vão para a UI formatados em pt-BR.
//!
//! Dividido por responsabilidade:
//! - `core`: orquestrador (`setup_cash`), refresh e helpers de valor/tempo
//! - `view`: render do estado na UI (sessão, movimentos, totais por método)
//! - `ops`: abrir/sangria/suprimento/fechar caixa

mod core;
mod ops;
mod view;

pub(crate) use core::setup_cash;
