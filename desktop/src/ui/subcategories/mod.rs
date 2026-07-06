//! Callbacks da tela de Subcategorias. AI_RULES.md §1, §8, §11.
//!
//! - `list`: refresh (join de nomes de categoria) e validação de form
//! - `crud`: criar/atualizar/excluir/reordenar

mod crud;
mod list;

pub(crate) use list::setup_refresh_subcategories;
pub(crate) use crud::{
    setup_add_subcategory, setup_delete_subcategory, setup_reorder_subcategory,
    setup_update_subcategory,
};
