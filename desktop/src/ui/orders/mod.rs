//! Callbacks da UI Slint para o domínio Pedidos.
//!
//! Módulo dividido por responsabilidade única (AI_RULES.md §8, §9) —
//! o arquivo único anterior havia crescido demais:
//! - `list`: refresh, kanban, status e detalhe do pedido
//! - `config`: configurador de produto (variações + adicionais)
//! - `calendar`: seletor de data/intervalo e tempo decorrido
//! - `edit`: edição de itens de um pedido existente
//! - `receipt`: impressão de comanda (cliente/cozinha)
//!
//! Re-exporta a API consumida por `ui::setup_callbacks` (callbacks
//! `setup_*`), por `ui::printers` (`send_to_default_printer`) e os
//! helpers puros reusados por `crate::print::pdf`, preservando os
//! caminhos `orders::*` usados externamente.

mod calendar;
mod config;
mod edit;
mod list;
mod receipt;

// Callbacks consumidos por `ui::setup_callbacks`.
pub(crate) use calendar::{setup_calendar, setup_refresh_order_elapsed};
pub(crate) use config::{
    setup_config_cancel, setup_config_confirm, setup_config_dec_qty, setup_config_inc_qty,
    setup_config_toggle_addon, setup_config_toggle_variation, setup_edit_order_edit_item,
    setup_start_product_config,
};
pub(crate) use edit::{
    setup_edit_order, setup_edit_order_add_product, setup_edit_order_dec, setup_edit_order_delete,
    setup_edit_order_filter_picker, setup_edit_order_inc, setup_save_edit_order,
};
pub(crate) use list::{
    active_orders_count, setup_advance_order_status, setup_cancel_order, setup_open_order,
    setup_refresh_orders,
};
pub(crate) use receipt::{send_to_default_printer, setup_print_receipt_now};

// Helpers puros reaproveitados por `crate::print::pdf` (geração da comanda).
pub(crate) use config::{format_addons_summary, format_qty};
pub(crate) use edit::strip_address_prefix;
pub(crate) use list::{format_elapsed_since, parse_address_parts};
pub(crate) use receipt::extract_address_for_print;
