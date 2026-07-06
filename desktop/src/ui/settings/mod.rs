//! Callbacks da tela de Configurações. AI_RULES.md §1, §8, §11, §14.
//!
//! - `hours`: horários de funcionamento, máscara de hora e override de loja
//! - `store`: dados do estabelecimento (nome/endereço/telefone, logo, capa)

mod hours;
mod store;

pub(crate) use hours::{
    setup_apply_time_mask, setup_refresh_business_hours, setup_save_business_hours,
    setup_set_store_override,
};
pub(crate) use store::{setup_pick_store_cover, setup_pick_store_logo, setup_save_store_info};
