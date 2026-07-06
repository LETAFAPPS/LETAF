//! Callbacks da tela de Banners. AI_RULES.md §1, §8, §11.
//!
//! - `crud`: refresh, imagem, criar/atualizar/alternar ativo e filtro
//! - `form`: validação de URL/form e conversão para a UI

mod crud;
mod form;

pub(crate) use crud::{
    setup_add_banner, setup_filter_banner_products, setup_pick_banner_image,
    setup_refresh_banners, setup_toggle_banner_active, setup_update_banner,
};
