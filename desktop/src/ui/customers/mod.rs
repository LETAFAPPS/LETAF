//! Callbacks da tela de Clientes.
//!
//! Dividido por responsabilidade (AI_RULES.md §8, §9):
//! - `data`: tipos decodificados (`DecodedCustomer`) e helpers de formatação
//! - `list`: refresh, agregação de métricas, filtro e seleção
//! - `crud`: validação/form, criar/atualizar/excluir, endereços e máscaras

mod crud;
mod data;
mod list;

pub(crate) use data::DecodedCustomer;
pub(crate) use list::{setup_filter_customers, setup_refresh_customers, setup_select_customer};
pub(crate) use crud::{
    setup_add_customer, setup_customer_address_ops, setup_delete_customer,
    setup_format_customer_fields, setup_update_customer,
};
