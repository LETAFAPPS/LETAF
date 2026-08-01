//! Callbacks da tela "Relatórios" (Central de análises).
//!
//! 4 sub-relatórios: financial, orders, products, customers.
//! Períodos: daily, weekly, monthly, yearly (sempre o período CORRENTE).
//!
//! AI_RULES §1/§3/§14: TODO o cálculo (DRE, margens, ticket médio,
//! rankings, janela de período) vive em `letaf_core::report`; esta
//! camada só busca dados, formata e pinta.
//!
//! Dividido por responsabilidade (§8, §9):
//! - `state`: estado da tela, granularidade e caches compartilhados
//! - `setup`: orquestrador (`setup_reports`), refresh e callbacks de filtro
//! - `snapshot`: monta o retrato (`build_snapshot`) e aplica na UI (`apply_to_ui`)
//! - `sections`: builders de cada sub-relatório (financial/orders/products/customers)
//! - `helpers`: rótulos, buckets do gráfico e formatação

mod helpers;
mod sections;
mod setup;
mod snapshot;
mod state;

pub(crate) use setup::setup_reports;
