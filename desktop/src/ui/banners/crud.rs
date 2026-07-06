use std::sync::Arc;

use slint::{ComponentHandle, Image, Model, ModelRc, SharedString, VecModel};
use tokio::sync::Notify;
use uuid::Uuid;


use crate::context::DesktopState;
use crate::{BannerData, MainWindow};

use super::super::helpers::show_toast;
use super::super::image::{decode_pixel_buffer, pick_image_file, process_image_file};
use super::form::{clear_form, read_and_validate, to_banner_data};

/// Validação de URL para `banner.item_url`.
/// - Precisa começar com `http://` ou `https://`.
/// - Precisa ter um domínio com TLD de 2+ letras (.com, .com.br, .app, etc.).
///
/// Retorna `None` se válido; `Some(msg)` com a razão se inválido.
pub(crate) fn validate_url(raw: &str) -> Option<&'static str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Some("Informe a URL");
    }
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        return Some("URL deve começar com http:// ou https://");
    }
    let after_scheme = trimmed.split_once("://").map(|x| x.1).unwrap_or("");
    let domain = after_scheme.split('/').next().unwrap_or("");
    let tld = domain.rsplit('.').next().unwrap_or("");
    let valid_tld = !domain.is_empty()
        && domain.contains('.')
        && tld.len() >= 2
        && tld.chars().all(|c| c.is_ascii_alphabetic());
    if !valid_tld {
        return Some("URL precisa de domínio válido (ex.: .com, .com.br, .app)");
    }
    None
}

/// Carrega banners do SQLite e injeta na UI (também resolve o
/// nome do produto vinculado a partir da lista atual de produtos).
pub(crate) fn setup_refresh_banners(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_refresh_banners(move || {
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            match state.banner_service.find_all(cid).await {
                Ok(items) => {
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        let products = ui.get_products();
                        let active = items.iter().filter(|b| b.active).count() as i32;
                        let inactive = items.len() as i32 - active;
                        let data: Vec<BannerData> = items.iter()
                            .map(|b| to_banner_data(b, &products))
                            .collect();
                        ui.set_banners(ModelRc::new(VecModel::from(data)));
                        ui.set_banners_active_count(active);
                        ui.set_banners_inactive_count(inactive);
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

/// Abre o seletor de arquivo, encoda em base64, atualiza preview.
pub(crate) fn setup_pick_banner_image(
    ui: &MainWindow,
    handle: &tokio::runtime::Handle,
) {
    let ui_weak = ui.as_weak();
    let handle = handle.clone();

    ui.on_pick_banner_image(move || {
        let ui_weak = ui_weak.clone();
        handle.spawn_blocking(move || {
            let Some(path) = pick_image_file() else { return };
            let uw = ui_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = uw.upgrade() { ui.set_banner_image_loading(true); }
            });
            match process_image_file(&path) {
                Some(b64) => {
                    let pixel_buf = decode_pixel_buffer(&b64);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak.upgrade() {
                            ui.set_banner_image_data(SharedString::from(b64));
                            if let Some(pb) = pixel_buf {
                                ui.set_banner_image(Image::from_rgba8(pb));
                            }
                            ui.set_banner_image_loading(false);
                            ui.set_banner_error_image(SharedString::default());
                        }
                    });
                }
                None => {
                    tracing::error!("Failed to process banner image: {}", path.display());
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak.upgrade() { ui.set_banner_image_loading(false); }
                    });
                }
            }
        });
    });
}

pub(crate) fn setup_add_banner(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_add_banner(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        let Some(form) = read_and_validate(&ui_ref) else { return };
        let ui_weak = ui_ref.as_weak();
        let state = state.clone();
        let notify = sync_notify.clone();

        handle.spawn(async move {
            let cid = state.company_id();
            match state.banner_service
                .create(cid, form.title, form.image_data, form.item_type, form.item_id, form.item_url)
                .await
            {
                Ok(_) => {
                    notify.notify_one();
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        show_toast(&ui, "Banner Criado", "success");
                        clear_form(&ui);
                        ui.set_show_modal(false);
                        ui.set_status_message(SharedString::from("Banner Criado"));
                        ui.invoke_refresh_banners();
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

pub(crate) fn setup_update_banner(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_update_banner(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        let id_str = ui_ref.get_editing_id().to_string();
        let Ok(id) = Uuid::parse_str(&id_str) else { return };
        let Some(form) = read_and_validate(&ui_ref) else { return };
        let ui_weak = ui_ref.as_weak();
        let state = state.clone();
        let notify = sync_notify.clone();

        handle.spawn(async move {
            let cid = state.company_id();
            match state.banner_service
                .update(cid, id, form.title, form.image_data, form.item_type, form.item_id, form.item_url)
                .await
            {
                Ok(_) => {
                    notify.notify_one();
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        show_toast(&ui, "Banner Atualizado", "success");
                        clear_form(&ui);
                        ui.set_show_modal(false);
                        ui.set_status_message(SharedString::from("Banner Atualizado"));
                        ui.invoke_refresh_banners();
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

/// Filtra a lista de produtos para o dropdown de busca do modal.
///
/// Lê `banner-product-search` da UI, normaliza para lowercase e
/// devolve a sub-lista cujo nome contém o termo. Chamado via
/// callback do `TextInput` (Slint não tem `string.contains`).
pub(crate) fn setup_filter_banner_products(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_filter_banner_products(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        let query = ui.get_banner_product_search().to_string().to_lowercase();
        let all = ui.get_products();
        let filtered: Vec<crate::ProductData> = if query.is_empty() {
            all.iter().collect()
        } else {
            all.iter()
                .filter(|p| p.name.to_lowercase().contains(&query))
                .collect()
        };
        ui.set_banner_product_filtered(ModelRc::new(VecModel::from(filtered)));
    });
}

pub(crate) fn setup_toggle_banner_active(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_toggle_banner_active(move |id_str| {
        let Ok(id) = Uuid::parse_str(id_str.as_str()) else { return };
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        let new_active = !ui_ref.get_banners().iter()
            .find(|b| b.id == id_str)
            .map(|b| b.active)
            .unwrap_or(true);

        let ui_weak = ui_ref.as_weak();
        let state = state.clone();
        let notify = sync_notify.clone();

        handle.spawn(async move {
            let cid = state.company_id();
            match state.banner_service.set_active(cid, id, new_active).await {
                Ok(()) => {
                    notify.notify_one();
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        let label = if new_active { "Banner Ativado" } else { "Banner Desativado" };
                        show_toast(&ui, label, "success");
                        ui.invoke_refresh_banners();
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

