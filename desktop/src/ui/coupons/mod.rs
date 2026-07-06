//! Callbacks da tela de Cupons.
//!
//! Dividido por responsabilidade (AI_RULES.md §8, §9):
//! - `crud`: refresh, criar/atualizar/alternar ativo + helper de máscara
//! - `form`: leitura/validação do formulário e formatação de exibição
//! - `cal`: calendário pop-up dos campos de validade

mod cal;
mod crud;
mod form;

pub(crate) use crud::{
    setup_add_coupon, setup_coupon_helpers, setup_refresh_coupons, setup_toggle_coupon_active,
    setup_update_coupon,
};
pub(crate) use cal::setup_coupon_cal;
