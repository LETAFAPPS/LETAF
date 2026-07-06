//! Callbacks da tela "Controle de Estoque" (Kanban por status).
//!
//! AI_RULES.md §1, §8, §11, §14: UI dispara callbacks; Rust faz I/O,
//! derivação de métricas e formatação. Acesso a dados só via service.
//!
//! - `setup`: orquestrador, listeners, busca, refresh e ajuste de estoque
//! - `view`: derivações e pintura das linhas do Kanban

mod setup;
mod view;

pub(crate) use setup::{out_of_stock_count, setup_inventory};
