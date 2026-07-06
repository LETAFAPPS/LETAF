//! Callbacks da UI Slint para o desktop.
//!
//! Estrutura modular por domínio (AI_RULES.md §8, §9):
//! - `helpers`: utilitários compartilhados (toast)
//! - `image`: codec/redimensionamento de imagens
//! - `auth`: login, logout, dark mode
//! - `products`, `customers`, `categories`, `subcategories`, `orders`, `settings`
//!
//! `setup_callbacks` é o único ponto público — conecta todos os callbacks
//! do Slint aos services do domínio, respeitando o isolamento por
//! `company_id` (§ multi-tenant) e o fluxo offline-first (§7).

mod addons;
mod admin;
mod alarm;
mod auth;
mod banners;
mod badges;
mod cash;
mod coupons;
mod categories;
mod collaborators;
mod dashboard;
mod customers;
mod finance;
mod helpers;
mod image;
mod inventory;
pub(crate) mod orders;
mod pdv;
mod printers;
mod products;
mod reports;
mod settings;
mod subcategories;
mod subscription;
mod sync;
mod wallet;

use std::sync::Arc;

use slint::{ComponentHandle, SharedString};
use tokio::sync::{Notify, RwLock};
use uuid::Uuid;

use crate::MainWindow;
use crate::context::DesktopState;

use self::helpers::show_toast;
use self::products::{DecodedProduct, remove_from_cache, remove_product_from_model};
use self::customers::DecodedCustomer;

/// Conecta callbacks da UI Slint aos services do dominio.
///
/// Regras aplicadas (AI_RULES.md §1, §3, §11, §14):
/// - UI nunca contem logica de negocio
/// - Callbacks delegam ao service via tokio runtime
/// - Nenhum acesso direto ao banco
/// - Login grava JWT no auth_token compartilhado com SyncWorker
#[allow(clippy::too_many_arguments)]
pub fn setup_callbacks(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
    sync_cycle_done: Arc<Notify>,
    badges_dirty: Arc<Notify>,
    auth_token: Arc<RwLock<Option<String>>>,
    server_url: String,
) {
    auth::setup_login(ui, state, handle, auth_token.clone(), sync_notify.clone(), server_url.clone());
    auth::setup_password_recovery(ui, handle, server_url.clone());
    auth::setup_profile(ui, state, handle, auth_token.clone(), server_url.clone());

    let products_cache: Arc<std::sync::Mutex<Vec<DecodedProduct>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let products_filter: products::SharedFilter =
        Arc::new(std::sync::Mutex::new(products::ProductFilterState::default()));
    products::setup_refresh(ui, state, handle, products_cache.clone(), products_filter.clone());
    products::setup_filter_products(ui, products_cache.clone(), products_filter.clone());
    products::setup_load_product_availability(ui);
    products::setup_load_product_addon_groups(ui, state, handle);
    products::setup_load_product_variations(ui);
    products::setup_add_variation(ui);
    products::setup_remove_variation(ui);
    products::setup_add_variation_option(ui);
    products::setup_remove_variation_option(ui);
    products::init_product_availability_default(ui);
    products::setup_add_discount_tier(ui);
    products::setup_remove_discount_tier(ui);
    products::setup_load_discount_tiers(ui);
    products::setup_toggle_category_filter(ui, products_cache.clone(), products_filter.clone());
    products::setup_toggle_subcategory_filter(ui, products_cache.clone(), products_filter.clone());
    products::setup_set_status_filter(ui, products_cache.clone(), products_filter.clone());
    products::setup_set_stock_filter(ui, products_cache.clone(), products_filter.clone());
    products::setup_reset_product_filters(ui, products_cache.clone(), products_filter.clone());
    products::setup_add(ui, state, handle, sync_notify.clone(), products_cache.clone());
    products::setup_update_product(ui, state, handle, sync_notify.clone(), products_cache.clone());
    products::setup_pick_product_image(ui, handle, products_cache.clone());
    products::setup_toggle_product_active(ui, state, handle, sync_notify.clone(), products_cache.clone());
    products::setup_toggle_web_visible(ui, state, handle, sync_notify.clone(), products_cache.clone());
    products::setup_delete(ui, state, handle, sync_notify.clone());
    products::setup_select_product(ui, products_cache.clone());
    products::setup_clear_detail_product(ui);
    products::setup_duplicate_product(ui, state, handle, sync_notify.clone(), products_cache.clone());
    // Listener leve do worker — atualiza o rótulo "Sincronizado" em
    // tempo real (sem re-decodificar imagens).
    products::setup_sync_listener(ui, state, handle, products_cache.clone(), sync_cycle_done.clone());

    setup_confirm_delete(ui, state, handle, sync_notify.clone(), products_cache.clone());

    let customers_cache: Arc<std::sync::Mutex<Vec<DecodedCustomer>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    customers::setup_refresh_customers(ui, state, handle, customers_cache.clone());
    customers::setup_filter_customers(ui, customers_cache.clone());
    customers::setup_select_customer(ui, customers_cache);
    customers::setup_add_customer(ui, state, handle, sync_notify.clone());
    customers::setup_update_customer(ui, state, handle, sync_notify.clone());
    customers::setup_delete_customer(ui, state, handle, sync_notify.clone());
    customers::setup_customer_address_ops(ui, state, handle, sync_notify.clone());
    customers::setup_format_customer_fields(ui);

    categories::load_category_icon_options(ui);
    categories::setup_categories(ui, state, handle);
    collaborators::setup_collaborators(ui, state, handle, sync_notify.clone());
    categories::setup_add_category(ui, state, handle, sync_notify.clone());
    categories::setup_update_category(ui, state, handle, sync_notify.clone());
    categories::setup_delete_category(ui, state, handle, sync_notify.clone());
    categories::setup_reorder_category(ui, state, handle, sync_notify.clone());

    subcategories::setup_refresh_subcategories(ui, state, handle);
    subcategories::setup_add_subcategory(ui, state, handle, sync_notify.clone());
    subcategories::setup_update_subcategory(ui, state, handle, sync_notify.clone());
    subcategories::setup_delete_subcategory(ui, state, handle, sync_notify.clone());
    subcategories::setup_reorder_subcategory(ui, state, handle, sync_notify.clone());

    // ── Adicionais (Fase 4B) ───────────────────────────
    addons::setup_refresh_addon_groups(ui, state, handle);
    addons::setup_select_addon_group(ui, state, handle);
    addons::setup_save_addon_group(ui, state, handle, sync_notify.clone());
    addons::setup_delete_addon_group(ui, state, handle, sync_notify.clone());
    addons::setup_save_addon(ui, state, handle, sync_notify.clone());
    addons::setup_delete_addon(ui, state, handle, sync_notify.clone());
    addons::setup_toggle_addon_active(ui, state, handle, sync_notify.clone());
    addons::setup_toggle_product_addon_group(ui);

    // ── Banners (Fase 7) ──────────────────────────────
    banners::setup_refresh_banners(ui, state, handle);
    banners::setup_filter_banner_products(ui);
    banners::setup_pick_banner_image(ui, handle);
    banners::setup_add_banner(ui, state, handle, sync_notify.clone());
    banners::setup_update_banner(ui, state, handle, sync_notify.clone());
    banners::setup_toggle_banner_active(ui, state, handle, sync_notify.clone());

    // ── Cupons (Fase 8) ───────────────────────────────
    coupons::setup_coupon_helpers(ui);
    coupons::setup_refresh_coupons(ui, state, handle);
    coupons::setup_add_coupon(ui, state, handle, sync_notify.clone());
    coupons::setup_update_coupon(ui, state, handle, sync_notify.clone());
    coupons::setup_toggle_coupon_active(ui, state, handle, sync_notify.clone());
    coupons::setup_coupon_cal(ui);

    orders::setup_refresh_orders(ui, state, handle);
    orders::setup_calendar(ui);
    orders::setup_open_order(ui, state, handle);
    orders::setup_advance_order_status(ui, state, handle, sync_notify.clone());
    orders::setup_cancel_order(ui, state, handle, sync_notify.clone());
    orders::setup_refresh_order_elapsed(ui);
    orders::setup_edit_order(ui, state, handle);
    orders::setup_edit_order_inc(ui);
    orders::setup_edit_order_dec(ui);
    orders::setup_edit_order_delete(ui);
    orders::setup_edit_order_edit_item(ui, state, handle);
    orders::setup_edit_order_add_product(ui);
    orders::setup_edit_order_filter_picker(ui);
    orders::setup_start_product_config(ui, state, handle);
    orders::setup_config_toggle_variation(ui);
    orders::setup_config_toggle_addon(ui);
    orders::setup_config_inc_qty(ui);
    orders::setup_config_dec_qty(ui);
    orders::setup_config_confirm(ui);
    orders::setup_config_cancel(ui);
    orders::setup_save_edit_order(ui, state, handle, sync_notify.clone());
    orders::setup_print_receipt_now(ui, state, handle);

    // Alarme de novos pedidos — observer + callbacks de UI (modal).
    alarm::setup_alarm(ui, state, handle);

    // Cadastro de impressoras (Configurações).
    printers::setup_printers(ui, state, handle);

    // PDV (Ponto de Venda).
    pdv::setup_pdv(ui, state, handle, sync_notify.clone());

    // Caixa (gestão de sessão).
    cash::setup_cash(ui, state, handle, sync_notify.clone());

    // Controle de Estoque.
    inventory::setup_inventory(ui, state, handle, sync_notify.clone(), sync_cycle_done.clone());

    // Dashboard
    dashboard::setup_dashboard(ui, state, handle, sync_cycle_done.clone());

    // Relatórios
    reports::setup_reports(ui, state, handle, sync_cycle_done.clone());

    // Financeiro (Fase 11)
    finance::setup_finance(ui, state, handle, sync_notify.clone(), sync_cycle_done.clone());

    // Carteira do cliente (Fase 12)
    wallet::setup_wallet(ui, state, handle, sync_notify.clone(), sync_cycle_done.clone());

    // Assinatura (Fase 13 — Plano & cobrança)
    subscription::setup_subscription(
        ui,
        state,
        handle,
        sync_notify.clone(),
        sync_cycle_done.clone(),
        auth_token.clone(),
        server_url.clone(),
    );

    settings::setup_refresh_business_hours(ui, state, handle);
    settings::setup_save_business_hours(ui, state, handle, sync_notify.clone());
    settings::setup_set_store_override(ui, state, handle, sync_notify.clone());
    settings::setup_save_store_info(ui, state, handle, sync_notify.clone());
    settings::setup_pick_store_logo(ui, handle);
    settings::setup_pick_store_cover(ui, handle);
    settings::setup_apply_time_mask(ui);

    auth::setup_logout(ui, state, handle, auth_token.clone());
    auth::setup_dark_mode(ui, state, handle);

    // Painel do administrador (super admin) — callbacks online /admin/*.
    admin::setup_admin(ui, handle, auth_token.clone(), server_url.clone());

    // Status do SyncWorker → propriedades da sidebar (polling 1.5s).
    // Também detecta invalidação de sessão (401 limpa o token) e força logout.
    sync::start_sync_status_timer(ui, state.sync_status.clone(), auth_token);

    // Badges da sidebar em TEMPO REAL: um único ouvinte recalcula
    // Pedidos, Financeiro (vencidas), Estoque (esgotados) e Assinatura
    // a cada ciclo de sync (toda escrita local dispara um). Pinta já no
    // startup para aparecerem sem abrir as abas.
    badges::setup_badges_listener(ui, state, handle, badges_dirty);
}

/// Callback: confirma exclusão — despacha para o service correto com base no tipo.
///
/// Regras aplicadas (AI_RULES.md §7.3, §7.4, §8):
/// - Lê tipo e ID do alvo da UI
/// - Delega ao service correto (product, category, subcategory, customer)
/// - Após exclusão, dispara sync e atualiza lista
///
/// Vive no `mod.rs` porque coordena múltiplos domínios — não pertence a um
/// único submódulo.
fn setup_confirm_delete(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
    products_cache: Arc<std::sync::Mutex<Vec<DecodedProduct>>>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_confirm_delete(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        let target_type = ui_ref.get_delete_target_type().to_string();
        let id_str = ui_ref.get_delete_target_id().to_string();

        let id = match Uuid::parse_str(&id_str) {
            Ok(id) => id,
            Err(e) => {
                tracing::error!("Invalid delete ID: {e}");
                return;
            }
        };

        let id_ss = SharedString::from(id_str.as_str());
        let ui_weak = ui_ref.as_weak();
        let state = state.clone();
        let notify = sync_notify.clone();
        let products_cache = products_cache.clone();

        handle.spawn(async move {
            let company_id = state.company_id();
            let result = match target_type.as_str() {
                "product" => state.product_service.soft_delete(company_id, id).await,
                "category" => state.category_service.soft_delete(company_id, id).await,
                "subcategory" => state.subcategory_service.soft_delete(company_id, id).await,
                "customer" => state.customer_service.soft_delete(company_id, id).await,
                "addon-group" => state.addon_group_service.soft_delete(company_id, id).await,
                "addon" => state.addon_service.soft_delete(company_id, id).await,
                "banner" => state.banner_service.soft_delete(company_id, id).await,
                "coupon" => state.coupon_service.soft_delete(company_id, id).await,
                "job-role" => state.job_role_service.soft_delete(company_id, id).await,
                "employee" => state.auth_service.soft_delete(company_id, id).await,
                _ => {
                    tracing::error!("Unknown delete target type: {target_type}");
                    return;
                }
            };

            if result.is_ok() { notify.notify_one(); }

            let target_type = target_type.clone();
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                match result {
                    Ok(()) => {
                        let label = match target_type.as_str() {
                            "product" => "Produto Excluído",
                            "category" => "Categoria Excluída",
                            "subcategory" => "Subcategoria Excluída",
                            "customer" => "Cliente Excluído",
                            "addon-group" => "Grupo de Adicionais Excluído",
                            "addon" => "Adicional Excluído",
                            "banner" => "Banner Excluído",
                            "coupon" => "Cupom Excluído",
                            "job-role" => "Função Excluída",
                            "employee" => "Funcionário Excluído",
                            _ => "Item Excluído",
                        };
                        show_toast(&ui, label, "success");
                        ui.set_status_message(SharedString::from(label));
                        match target_type.as_str() {
                            "product" => {
                                remove_product_from_model(&ui, &id_ss);
                                remove_from_cache(&products_cache, id_ss.as_str());
                                // Limpa o detalhe quando o produto
                                // excluído era o selecionado.
                                if ui.get_selected_product_id() == id_ss {
                                    ui.set_selected_product_id(SharedString::default());
                                    ui.set_detail_product(crate::ProductData::default());
                                }
                            }
                            "category" => ui.invoke_refresh_categories(),
                            "subcategory" => {
                                ui.invoke_refresh_subcategories();
                                ui.invoke_refresh_categories();
                            }
                            "customer" => ui.invoke_refresh_customers(),
                            "addon-group" => {
                                if ui.get_selected_addon_group_id() == id_ss {
                                    ui.set_selected_addon_group_id(SharedString::default());
                                    ui.set_selected_addon_group_name(SharedString::default());
                                }
                                ui.invoke_refresh_addon_groups();
                            }
                            "addon" => {
                                let gid = ui.get_selected_addon_group_id();
                                ui.invoke_refresh_addon_groups();
                                if !gid.is_empty() { ui.invoke_select_addon_group(gid); }
                            }
                            "banner" => ui.invoke_refresh_banners(),
                            "coupon" => ui.invoke_refresh_coupons(),
                            "job-role" | "employee" => ui.invoke_refresh_collaborators(),
                            _ => {}
                        }
                    }
                    Err(e) => {
                        let msg = format!("Erro: {e}");
                        show_toast(&ui, &msg, "error");
                        ui.set_status_message(SharedString::from(msg));
                    }
                }
            });
        });
    });
}
