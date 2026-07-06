//! Callbacks da tela de Categorias (master-detail).
//!
//! Dividido por responsabilidade (AI_RULES.md §8, §9):
//! - `render`: cache (`CatCache`), árvore, agregação do detalhe e cores
//! - `setup`: registro do master-detail (`setup_categories`)
//! - `crud`: validação/form, criar/atualizar/excluir/reordenar, ícones

mod crud;
mod render;
mod setup;

pub(crate) use setup::setup_categories;
pub(crate) use crud::{
    load_category_icon_options, setup_add_category, setup_delete_category, setup_reorder_category,
    setup_update_category,
};
