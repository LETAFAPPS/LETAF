use std::sync::Arc;

use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use tokio::sync::Notify;
use uuid::Uuid;


use crate::context::DesktopState;
use crate::format::format_date_br;
use crate::{CouponData, MainWindow};

use super::super::helpers::show_toast;
use super::cal::to_coupon_data;
use super::form::{clear_form, read_and_validate, report_error};

/// Registra a máscara de data DD/MM/AAAA dos campos de validade.
pub(crate) fn setup_coupon_helpers(ui: &MainWindow) {
    ui.on_format_coupon_date(|raw| SharedString::from(format_date_br(raw.as_str())));
}

/// Carrega cupons do SQLite e injeta na UI (lista + contadores).
pub(crate) fn setup_refresh_coupons(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_refresh_coupons(move || {
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            match state.coupon_service.find_all(cid).await {
                Ok(items) => {
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        let active = items.iter().filter(|c| c.active).count() as i32;
                        let inactive = items.len() as i32 - active;
                        let data: Vec<CouponData> = items.iter().map(to_coupon_data).collect();
                        ui.set_coupons(ModelRc::new(VecModel::from(data)));
                        ui.set_coupons_active_count(active);
                        ui.set_coupons_inactive_count(inactive);
                    });
                }
                Err(e) => {
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        ui.set_status_message(SharedString::from(format!("Erro: {e}")));
                    });
                }
            }
        });
    });
}

pub(crate) fn setup_add_coupon(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_add_coupon(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        let Some(f) = read_and_validate(&ui_ref) else { return };
        let ui_weak = ui_ref.as_weak();
        let state = state.clone();
        let notify = sync_notify.clone();

        handle.spawn(async move {
            let cid = state.company_id();
            match state.coupon_service.create(
                cid, f.title, f.code, f.coupon_type, f.discount_kind, f.discount_value,
                f.min_order_value, f.max_discount, f.per_user_limit, f.usage_limit,
                f.valid_from, f.valid_until,
            ).await {
                Ok(_) => {
                    notify.notify_one();
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        show_toast(&ui, "Cupom Criado", "success");
                        clear_form(&ui);
                        ui.set_show_modal(false);
                        ui.set_status_message(SharedString::from("Cupom Criado"));
                        ui.invoke_refresh_coupons();
                    });
                }
                Err(e) => report_error(ui_weak, e),
            }
        });
    });
}

pub(crate) fn setup_update_coupon(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_update_coupon(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        let id_str = ui_ref.get_editing_id().to_string();
        let Ok(id) = Uuid::parse_str(&id_str) else { return };
        let Some(f) = read_and_validate(&ui_ref) else { return };
        let ui_weak = ui_ref.as_weak();
        let state = state.clone();
        let notify = sync_notify.clone();

        handle.spawn(async move {
            let cid = state.company_id();
            match state.coupon_service.update(
                cid, id, f.title, f.code, f.coupon_type, f.discount_kind, f.discount_value,
                f.min_order_value, f.max_discount, f.per_user_limit, f.usage_limit,
                f.valid_from, f.valid_until,
            ).await {
                Ok(_) => {
                    notify.notify_one();
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        show_toast(&ui, "Cupom Atualizado", "success");
                        clear_form(&ui);
                        ui.set_show_modal(false);
                        ui.set_status_message(SharedString::from("Cupom Atualizado"));
                        ui.invoke_refresh_coupons();
                    });
                }
                Err(e) => report_error(ui_weak, e),
            }
        });
    });
}

pub(crate) fn setup_toggle_coupon_active(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_toggle_coupon_active(move |id_str| {
        let Ok(id) = Uuid::parse_str(id_str.as_str()) else { return };
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        let new_active = !ui_ref.get_coupons().iter()
            .find(|c| c.id == id_str)
            .map(|c| c.active)
            .unwrap_or(true);

        let ui_weak = ui_ref.as_weak();
        let state = state.clone();
        let notify = sync_notify.clone();

        handle.spawn(async move {
            let cid = state.company_id();
            match state.coupon_service.set_active(cid, id, new_active).await {
                Ok(()) => {
                    notify.notify_one();
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        let label = if new_active { "Cupom Ativado" } else { "Cupom Desativado" };
                        show_toast(&ui, label, "success");
                        ui.invoke_refresh_coupons();
                    });
                }
                Err(e) => {
                    let msg = format!("Erro: {e}");
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        show_toast(&ui, &msg, "error");
                    });
                }
            }
        });
    });
}

