//! Callbacks da tela Dashboard. Toda agregação no Rust; a UI só consome
//! listas/strings prontas (AI_RULES.md §1, §8, §11, §14).
//!
//! - `setup`: orquestrador (`setup_dashboard`), refresh e sync listener
//! - `snapshot`: agregação (KPIs, séries) e pintura na UI

mod setup;
mod snapshot;

pub(crate) use setup::setup_dashboard;
