//! Callbacks da tela "Relatórios" (Central de análises).
//!
//! 4 sub-relatórios: financial, orders, products, customers.
//! Períodos: 7d, 30d, month (mês corrente).
//!
//! AI_RULES §1/§11/§14: agregação em Rust, UI só pinta.
//!
//! Dividido por responsabilidade (§8, §9):
//! - `state`: estado da tela, granularidade e caches compartilhados
//! - `setup`: orquestrador (`setup_reports`), refresh e callbacks de filtro
//! - `snapshot`: agregação (`build_snapshot`) e aplicação na UI (`apply_to_ui`)
//! - `sections`: builders de cada sub-relatório (financial/orders/products/customers)
//! - `helpers`: funções puras de KPI, buckets diários/mensais e formatação

mod helpers;
mod sections;
mod setup;
mod snapshot;
mod state;

pub(crate) use setup::setup_reports;
