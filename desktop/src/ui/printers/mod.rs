//! Callbacks da seção "Impressoras" (Configurações). Impressora é local
//! ao desktop (não sincroniza). AI_RULES.md §1, §8, §11.
//!
//! - `crud`: cadastro (refresh, salvar, excluir, padrão) e categorias
//! - `print`: teste de impressão e enumeração de impressoras do sistema

mod crud;
mod print;

pub(crate) use crud::setup_printers;
