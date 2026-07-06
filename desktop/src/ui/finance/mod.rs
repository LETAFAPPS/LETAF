//! Callbacks da tela "Financeiro" (contas a pagar/receber).
//!
//! AI_RULES.md §1, §3, §8, §11, §14:
//! - UI dispara callbacks; Rust faz I/O, validação e formatação.
//! - Toda derivação (KPIs, filtros, status `Overdue`) acontece aqui.
//!
//! Dividido por responsabilidade (§8, §9):
//! - `state`: estado de navegação dos calendários e handles compartilhados
//! - `setup`: orquestrador (`setup_finance`), refresh e modais de confirmação
//! - `snapshot`: pipeline de agregação/render (snapshot → UI)
//! - `modal`: abas, busca/filtro, modal de lançamento e ações da entrada
//! - `calendar`: picker de cliente, calendário de vencimento e navegação
//! - `helpers`: funções puras de formatação/parse

mod calendar;
mod helpers;
mod modal;
mod setup;
mod snapshot;
mod state;

pub(crate) use setup::{overdue_count, setup_finance};
