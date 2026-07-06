//! Callbacks da UI Slint para o domínio Produtos.
//!
//! Módulo dividido por fase de responsabilidade (AI_RULES.md §8, §9) — o
//! arquivo único anterior (~2400 linhas) foi quebrado em:
//! - `state`: tipos/estado de UI (filtros, `DecodedProduct`) e parsing
//!   da agenda de disponibilidade
//! - `list`: lista mestre, seleção, cache decodificado e duplicação
//! - `editors`: editores interativos (disponibilidade, variações, faixas
//!   de desconto) e carregamento de grupos de adicionais
//! - `filter`: callbacks de filtro da grade
//! - `form`: leitura, validação e limpeza do formulário de produto
//! - `crud`: criar, atualizar e excluir
//! - `data`: helpers puros de display/decodificação e mutação do modelo
//! - `ops`: sync listener, imagem e toggles (ativo / visível na web)
//!
//! Re-exporta a API consumida por `ui::setup_callbacks`, preservando os
//! caminhos `products::*` usados externamente.

mod crud;
mod data;
mod editors;
mod filter;
mod form;
mod list;
mod ops;
mod state;

pub(crate) use state::{DecodedProduct, ProductFilterState, SharedFilter};

pub(crate) use list::{
    remove_from_cache, setup_clear_detail_product, setup_duplicate_product, setup_refresh,
    setup_select_product,
};
pub(crate) use editors::{
    init_product_availability_default, setup_add_discount_tier, setup_add_variation,
    setup_add_variation_option, setup_load_discount_tiers, setup_load_product_addon_groups,
    setup_load_product_availability, setup_load_product_variations, setup_remove_discount_tier,
    setup_remove_variation, setup_remove_variation_option,
};
pub(crate) use filter::{
    setup_filter_products, setup_reset_product_filters, setup_set_status_filter,
    setup_set_stock_filter, setup_toggle_category_filter, setup_toggle_subcategory_filter,
};
pub(crate) use crud::{setup_add, setup_delete, setup_update_product};
pub(crate) use ops::{
    remove_product_from_model, setup_pick_product_image, setup_sync_listener,
    setup_toggle_product_active, setup_toggle_web_visible,
};
