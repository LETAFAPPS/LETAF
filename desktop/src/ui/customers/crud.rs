use std::sync::Arc;

use slint::{ComponentHandle, SharedString};
use tokio::sync::Notify;
use uuid::Uuid;


use crate::context::DesktopState;
use crate::format::{format_document, format_phone};
use crate::{CustomerData, MainWindow};

use super::super::helpers::{show_toast, user_error};
use super::data::DecodedCustomer;

/// Valida formato de telefone: (XX) XXXX-XXXX ou (XX) XXXXX-XXXX.
pub(crate) fn is_valid_phone(phone: &str) -> bool {
    let digits: String = phone.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() != 10 && digits.len() != 11 { return false; }
    if phone.chars().all(|c| c.is_ascii_digit()) { return true; }
    let re_10 = phone.len() == 14 && phone.starts_with('(')
        && phone.as_bytes()[3] == b')' && phone.as_bytes()[4] == b' '
        && phone.as_bytes()[9] == b'-';
    let re_11 = phone.len() == 15 && phone.starts_with('(')
        && phone.as_bytes()[3] == b')' && phone.as_bytes()[4] == b' '
        && phone.as_bytes()[10] == b'-';
    re_10 || re_11
}

pub(crate) fn clear_customer_errors(ui: &MainWindow) {
    ui.set_customer_error_name(SharedString::default());
    ui.set_customer_error_email(SharedString::default());
    ui.set_customer_error_phone(SharedString::default());
    ui.set_customer_error_document(SharedString::default());
}

pub(crate) fn clear_customer_form(ui: &MainWindow) {
    ui.set_customer_name(SharedString::default());
    ui.set_customer_email(SharedString::default());
    ui.set_customer_phone(SharedString::default());
    ui.set_customer_document(SharedString::default());
    ui.set_customer_notes(SharedString::default());
    ui.set_customer_profile_picture(slint::Image::default());
    ui.set_customer_avatar_initial(SharedString::from("?"));
    clear_customer_errors(ui);
}

pub(crate) fn validate_customer_form(ui: &MainWindow) -> bool {
    let mut valid = true;
    clear_customer_errors(ui);

    if ui.get_customer_name().trim().is_empty() {
        ui.set_customer_error_name(SharedString::from("Preencha o nome do cliente"));
        valid = false;
    }
    let email = ui.get_customer_email().trim().to_string();
    if email.is_empty() {
        ui.set_customer_error_email(SharedString::from("Preencha o email"));
        valid = false;
    } else if !email.contains('@') || !email.contains('.') {
        ui.set_customer_error_email(SharedString::from("Email inválido"));
        valid = false;
    }
    let phone = ui.get_customer_phone().trim().to_string();
    if phone.is_empty() {
        ui.set_customer_error_phone(SharedString::from("Preencha o telefone"));
        valid = false;
    } else if !is_valid_phone(&phone) {
        ui.set_customer_error_phone(SharedString::from("Telefone inválido"));
        valid = false;
    }
    let document = ui.get_customer_document().trim().to_string();
    if !document.is_empty() {
        let digits: String = document.chars().filter(|c| c.is_ascii_digit()).collect();
        if digits.len() != 11 && digits.len() != 14 {
            ui.set_customer_error_document(SharedString::from(
                "CPF (11 dígitos) ou CNPJ (14 dígitos) incompleto",
            ));
            valid = false;
        }
    }
    valid
}

/// Callback: cria novo cliente no SQLite e dispara sync.
pub(crate) fn setup_add_customer(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_add_customer(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        if !validate_customer_form(&ui_ref) { return; }

        let name = ui_ref.get_customer_name().to_string();
        let email = ui_ref.get_customer_email().to_string();
        let phone = ui_ref.get_customer_phone().to_string();
        let document = ui_ref.get_customer_document().to_string();
        let notes = ui_ref.get_customer_notes().to_string();

        let ui_weak = ui_ref.as_weak();
        let state = state.clone();
        let notify = sync_notify.clone();

        handle.spawn(async move {
            let cid = state.company_id();
            let email_opt = if email.is_empty() { None } else { Some(email) };
            let phone_opt = if phone.is_empty() { None } else { Some(phone) };
            let doc_opt = if document.is_empty() { None } else { Some(document) };
            let notes_opt = if notes.trim().is_empty() { None } else { Some(notes) };

            match state.customer_service.create(cid, name, email_opt, phone_opt, doc_opt, notes_opt).await {
                Ok(_) => {
                    notify.notify_one();
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        clear_customer_form(&ui);
                        ui.set_show_modal(false);
                        ui.set_status_message(SharedString::from("Cliente Criado!"));
                        show_toast(&ui, "Cliente Criado!", "success");
                        ui.invoke_refresh_customers();
                    });
                }
                Err(e) => {
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        ui.set_status_message(SharedString::from(format!("Erro: {e}")));
                        show_toast(&ui, &user_error(&e), "error");
                    });
                }
            }
        });
    });
}

/// Callback: atualiza um cliente existente (inclui observação interna).
pub(crate) fn setup_update_customer(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_update_customer(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        if !validate_customer_form(&ui_ref) { return; }

        let id_str = ui_ref.get_editing_id().to_string();
        let Ok(id) = Uuid::parse_str(&id_str) else { return };
        let name = ui_ref.get_customer_name().to_string();
        let email = ui_ref.get_customer_email().to_string();
        let phone = ui_ref.get_customer_phone().to_string();
        let document = ui_ref.get_customer_document().to_string();
        let notes = ui_ref.get_customer_notes().to_string();

        let ui_weak = ui_ref.as_weak();
        let state = state.clone();
        let notify = sync_notify.clone();

        handle.spawn(async move {
            let cid = state.company_id();
            let email_opt = if email.is_empty() { None } else { Some(email) };
            let phone_opt = if phone.is_empty() { None } else { Some(phone) };
            let doc_opt = if document.is_empty() { None } else { Some(document) };
            let notes_opt = if notes.trim().is_empty() { None } else { Some(notes) };

            match state.customer_service
                .update(cid, id, name, email_opt, phone_opt, doc_opt, notes_opt).await
            {
                Ok(c) => {
                    notify.notify_one();
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        clear_customer_form(&ui);
                        ui.set_editing_id(SharedString::default());
                        ui.set_show_modal(false);
                        ui.set_status_message(SharedString::from(format!("Cliente '{}' Atualizado", c.name)));
                        show_toast(&ui, &format!("Cliente '{}' Atualizado", c.name), "success");
                        ui.invoke_refresh_customers();
                    });
                }
                Err(e) => {
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        ui.set_status_message(SharedString::from(format!("Erro: {e}")));
                        show_toast(&ui, &user_error(&e), "error");
                    });
                }
            }
        });
    });
}

/// Callback: soft-delete de cliente e dispara sync.
pub(crate) fn setup_delete_customer(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_delete_customer(move |id_str| {
        let Ok(id) = uuid::Uuid::parse_str(id_str.as_str()) else { return };
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let notify = sync_notify.clone();

        handle.spawn(async move {
            let cid = state.company_id();
            match state.customer_service.soft_delete(cid, id).await {
                Ok(_) => {
                    notify.notify_one();
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        show_toast(&ui, "Cliente Excluído", "success");
                        ui.set_status_message(SharedString::from("Cliente Excluído"));
                        ui.set_selected_customer_id(SharedString::default());
                        ui.invoke_refresh_customers();
                    });
                }
                Err(e) => {
                    let msg = format!("Erro: {e}");
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        show_toast(&ui, &msg, "error");
                        ui.set_status_message(SharedString::from(msg));
                    });
                }
            }
        });
    });
}

/// Callbacks de endereço do cliente (criar / remover no modal).
/// Persiste local primeiro (offline-first §7); o sync worker
/// empurra para o servidor, compartilhando com o web.
pub(crate) fn setup_customer_address_ops(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    // ── Adicionar endereço ──
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        let handle = handle.clone();
        let notify = sync_notify.clone();
        ui.on_add_customer_address(move || {
            let Some(uic) = ui_weak.upgrade() else { return };
            let Ok(customer_id) = Uuid::parse_str(uic.get_editing_id().as_str()) else { return };
            let label = uic.get_customer_addr_label().to_string();
            let custom = uic.get_customer_addr_custom_label().to_string();
            let custom_label = if label == "Outros" && !custom.trim().is_empty() {
                Some(custom)
            } else { None };
            let street = uic.get_customer_addr_street().to_string();
            let number = uic.get_customer_addr_number().to_string();
            let neighborhood = uic.get_customer_addr_neighborhood().to_string();
            let apt = uic.get_customer_addr_apartment().to_string();
            let apartment = if apt.trim().is_empty() { None } else { Some(apt) };
            let ui_weak = uic.as_weak();
            let state = state.clone();
            let notify = notify.clone();
            handle.spawn(async move {
                let cid = state.company_id();
                match state.customer_address_service
                    .create(cid, customer_id, label, custom_label, street, number, neighborhood, apartment)
                    .await
                {
                    Ok(_) => {
                        notify.notify_one();
                        let _ = slint::invoke_from_event_loop(move || {
                            let Some(ui) = ui_weak.upgrade() else { return };
                            ui.set_customer_addr_custom_label(SharedString::default());
                            ui.set_customer_addr_street(SharedString::default());
                            ui.set_customer_addr_number(SharedString::default());
                            ui.set_customer_addr_neighborhood(SharedString::default());
                            ui.set_customer_addr_apartment(SharedString::default());
                            show_toast(&ui, "Endereço adicionado", "success");
                            ui.invoke_refresh_customers();
                        });
                    }
                    Err(e) => {
                        let msg = format!("Erro: {e}");
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak.upgrade() { show_toast(&ui, &msg, "error"); }
                        });
                    }
                }
            });
        });
    }
    // ── Remover endereço ──
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        let handle = handle.clone();
        ui.on_delete_customer_address(move |addr_id| {
            let Some(uic) = ui_weak.upgrade() else { return };
            let Ok(customer_id) = Uuid::parse_str(uic.get_editing_id().as_str()) else { return };
            let Ok(aid) = Uuid::parse_str(addr_id.as_str()) else { return };
            let ui_weak = uic.as_weak();
            let state = state.clone();
            let notify = sync_notify.clone();
            handle.spawn(async move {
                let cid = state.company_id();
                match state.customer_address_service.soft_delete(cid, aid, customer_id).await {
                    Ok(()) => {
                        notify.notify_one();
                        let _ = slint::invoke_from_event_loop(move || {
                            let Some(ui) = ui_weak.upgrade() else { return };
                            show_toast(&ui, "Endereço removido", "success");
                            ui.invoke_refresh_customers();
                        });
                    }
                    Err(e) => {
                        let msg = format!("Erro: {e}");
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak.upgrade() { show_toast(&ui, &msg, "error"); }
                        });
                    }
                }
            });
        });
    }
}

/// Extrai o pixel buffer do data URL de perfil (spawn_blocking).
pub(crate) fn decode_customer_pixel_buffer(data_url: &str) -> Option<slint::SharedPixelBuffer<slint::Rgba8Pixel>> {
    let comma = data_url.find(',')?;
    let b64 = &data_url[comma + 1..];
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    let img = image::load_from_memory(&bytes).ok()?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    Some(slint::SharedPixelBuffer::clone_from_slice(rgba.as_raw(), w, h))
}

/// Converte &DecodedCustomer → CustomerData (sem consumir).
pub(crate) fn decoded_to_customer_data_ref(d: &DecodedCustomer) -> CustomerData {
    CustomerData {
        id: d.id.clone(),
        name: d.name.clone(),
        email: d.email.clone(),
        phone: d.phone.clone(),
        document: d.document.clone(),
        avatar_initial: d.avatar_initial.clone(),
        notes: d.notes.clone(),
        created_at: d.created_at.clone(),
        ltv: d.ltv.clone(),
        ltv_pct: d.ltv_pct.clone(),
        order_count: d.order_count,
        avg_ticket: d.avg_ticket.clone(),
        last_order: d.last_order.clone(),
        last_order_rel: d.last_order_rel.clone(),
        status: d.status.clone(),
        status_label: d.status_label.clone(),
        is_vip: d.is_vip,
        profile_picture: d.pixel_buffer.clone()
            .map(slint::Image::from_rgba8)
            .unwrap_or_default(),
    }
}

/// Registra callbacks de formatação de telefone e documento.
pub(crate) fn setup_format_customer_fields(ui: &MainWindow) {
    // "+ Novo pedido" — o desktop não cria pedidos (vêm do web).
    // Placeholder para a futura criação de pedido no balcão.
    {
        let uw = ui.as_weak();
        ui.on_new_customer_order(move |_id| {
            if let Some(ui) = uw.upgrade() {
                show_toast(&ui, "Criar pedido no balcão estará disponível em breve.", "info");
            }
        });
    }
    ui.on_format_customer_phone(|raw| SharedString::from(format_phone(raw.as_str())));
    ui.on_format_customer_document(|raw| SharedString::from(format_document(raw.as_str())));
    ui.on_format_store_phone(|raw| SharedString::from(format_phone(raw.as_str())));
    ui.on_format_store_document(|raw| SharedString::from(format_document(raw.as_str())));
    ui.on_format_store_zip(|raw| SharedString::from(crate::format::format_zip_code(raw.as_str())));
}
