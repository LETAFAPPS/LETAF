//! Callbacks do domínio Adicionais (Fase 4B).
//!
//! AI_RULES.md §1, §8, §11: UI só dispara callbacks; validação no service.
//!
//! - `groups`: grupos de adicionais (refresh, seleção, salvar, excluir)
//! - `items`: adicionais e vínculo produto↔grupo

mod groups;
mod items;

pub(crate) use groups::{
    setup_delete_addon_group, setup_refresh_addon_groups, setup_save_addon_group,
    setup_select_addon_group,
};
pub(crate) use items::{
    read_selected_addon_group_ids, refresh_product_addon_groups, setup_delete_addon,
    setup_save_addon, setup_toggle_addon_active, setup_toggle_product_addon_group,
};
