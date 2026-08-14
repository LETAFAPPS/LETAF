use std::sync::Arc;

use slint::{ComponentHandle, SharedString};
use tokio::sync::Notify;


use crate::context::DesktopState;
use crate::MainWindow;

use super::super::helpers::{friendly_error, show_toast};
use super::super::image::{decode_pixel_buffer, pick_image_file, process_image_file, process_image_file_large};
use crate::SettingsState;

/// Faz o parse de um valor monetário digitado (ex.: "5", "5,00", "R$ 5,00",
/// "1.234,56") para `Decimal`, sempre >= 0. Vírgula é o separador decimal
/// (pt-BR); pontos são tratados como separador de milhar quando há vírgula.
fn parse_money(s: &str) -> rust_decimal::Decimal {
    crate::format::parse_money_br(s)
        .unwrap_or(rust_decimal::Decimal::ZERO)
        .max(rust_decimal::Decimal::ZERO)
}

/// Callbacks do color picker de "Cores do site": converte matiz+tonalidade
/// em cor/hex (pick) e limpa (usar cores do tema). Só apresentação (§11) —
/// o hex é validado no backend ao salvar.
pub(crate) fn setup_site_color(ui: &MainWindow) {
    let ui_pick = ui.as_weak();
    ui.global::<SettingsState>().on_pick_site_color(move |hue, tone| {
        let Some(ui) = ui_pick.upgrade() else { return };
        let p = super::color::from_hue_tone(hue, tone);
        let s = ui.global::<SettingsState>();
        s.set_site_color_preview(p.preview);
        s.set_site_tone_mid(p.tone_mid);
        s.set_site_color_on(true);
        s.set_store_site_palette(SharedString::from(p.hex));
    });

    let ui_clear = ui.as_weak();
    ui.global::<SettingsState>().on_clear_site_color(move || {
        let Some(ui) = ui_clear.upgrade() else { return };
        let s = ui.global::<SettingsState>();
        s.set_site_color_on(false);
        s.set_store_site_palette(SharedString::from(""));
    });
}

/// Callback: salva informações do estabelecimento (nome, endereço, telefone, logo, capa).
pub(crate) fn setup_save_store_info(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.global::<SettingsState>().on_save_store_info(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        let name         = ui_ref.global::<SettingsState>().get_store_name().to_string();
        let address      = ui_ref.global::<SettingsState>().get_store_address().to_string();
        let phone_raw    = ui_ref.global::<SettingsState>().get_store_phone().to_string();
        let whatsapp_raw = ui_ref.global::<SettingsState>().get_store_whatsapp().to_string();
        let email        = ui_ref.global::<SettingsState>().get_store_email().to_string();
        let instagram    = ui_ref.global::<SettingsState>().get_store_instagram().to_string();
        let document_raw = ui_ref.global::<SettingsState>().get_store_document().to_string();
        let neighborhood = ui_ref.global::<SettingsState>().get_store_neighborhood().to_string();
        let zip_raw      = ui_ref.global::<SettingsState>().get_store_zip_code().to_string();
        let city         = ui_ref.global::<SettingsState>().get_store_city().to_string();
        let uf           = ui_ref.global::<SettingsState>().get_store_uf().to_string();
        let logo         = ui_ref.global::<SettingsState>().get_store_logo_data().to_string();
        let cover        = ui_ref.global::<SettingsState>().get_store_cover_data().to_string();
        let products_per_page = ui_ref.global::<SettingsState>().get_products_per_page();
        let orders_per_page   = ui_ref.global::<SettingsState>().get_orders_per_page();
        let delivery_fee_raw  = ui_ref.global::<SettingsState>().get_store_delivery_fee().to_string();
        let site_palette      = ui_ref.global::<SettingsState>().get_store_site_palette().to_string();
        let site_default_scheme = ui_ref.global::<SettingsState>().get_store_default_scheme().to_string();

        // Normalização defensiva: telefones/documentos/CEP guardados só
        // com dígitos no banco (formatação acontece na UI). Isso evita
        // duplicação de registros por máscaras diferentes.
        let only_digits = |s: &str| -> String { s.chars().filter(|c| c.is_ascii_digit()).collect() };
        let phone_digits    = only_digits(&phone_raw);
        let whatsapp_digits = only_digits(&whatsapp_raw);
        let document_digits = only_digits(&document_raw);
        let zip_digits      = only_digits(&zip_raw);
        let some_if_filled = |s: String| if s.is_empty() { None } else { Some(s) };

        let ui_weak2 = ui_ref.as_weak();
        let state = state.clone();
        let notify = sync_notify.clone();

        handle.spawn(async move {
            let cid = state.company_id();
            // Coordenadas não são editadas nesta tela; preserva as atuais
            // (definidas no painel do super admin) para não zerá-las no save.
            let current = state.company_service.find_by_id(cid).await.ok().flatten();
            let input = letaf_core::company::service::UpdateInfoInput {
                name,
                address: some_if_filled(address),
                phone: some_if_filled(phone_digits),
                whatsapp: some_if_filled(whatsapp_digits),
                email: some_if_filled(email),
                instagram: some_if_filled(instagram),
                document: some_if_filled(document_digits),
                neighborhood: some_if_filled(neighborhood),
                zip_code: some_if_filled(zip_digits),
                city: some_if_filled(city),
                uf: some_if_filled(uf),
                // Link da localização é definido no painel do super admin;
                // preserva o valor atual para não zerá-lo no save.
                location_url: current.as_ref().and_then(|c| c.location_url.clone()),
                logo_data: some_if_filled(logo),
                cover_data: some_if_filled(cover),
                products_per_page,
                orders_per_page,
                delivery_fee: parse_money(&delivery_fee_raw),
                // Paleta de cores do site ("" = Nenhuma → None). Validação do
                // slug fica no service (§11).
                color_palette: some_if_filled(site_palette),
                // Tema padrão do site ("" = Automático → None). Validação
                // ("light"/"dark") fica no service (§11).
                default_scheme: some_if_filled(site_default_scheme),
            };
            let result = state.company_service.update_info(cid, input).await;
            match result {
                Ok(_) => {
                    notify.notify_one();
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak2.upgrade() else { return };
                        show_toast(&ui, "Informações salvas com sucesso", "success");
                    });
                }
                Err(e) => {
                    let msg = format!("Erro ao salvar informações: {}", friendly_error(&e));
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak2.upgrade() else { return };
                        show_toast(&ui, &msg, "error");
                    });
                }
            }
        });
    });
}

/// Callback: abre seletor e processa imagem de logo do estabelecimento.
pub(crate) fn setup_pick_store_logo(
    ui: &MainWindow,
    handle: &tokio::runtime::Handle,
) {
    let ui_weak = ui.as_weak();
    let handle = handle.clone();

    ui.global::<SettingsState>().on_pick_store_logo(move || {
        let ui_weak = ui_weak.clone();
        handle.spawn_blocking(move || {
            let Some(path) = pick_image_file() else { return };
            let uw = ui_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = uw.upgrade() { ui.global::<SettingsState>().set_store_logo_loading(true); }
            });
            match process_image_file(&path) {
                Some(encoded) => {
                    let pixel_buf = decode_pixel_buffer(&encoded);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak.upgrade() {
                            ui.global::<SettingsState>().set_store_logo_image(pixel_buf.map(slint::Image::from_rgba8).unwrap_or_default());
                            ui.global::<SettingsState>().set_store_logo_data(SharedString::from(encoded));
                            ui.global::<SettingsState>().set_store_logo_loading(false);
                        }
                    });
                }
                None => {
                    tracing::error!("Failed to process logo: {}", path.display());
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak.upgrade() { ui.global::<SettingsState>().set_store_logo_loading(false); }
                    });
                }
            }
        });
    });
}

/// Callback: abre seletor e processa imagem de capa do estabelecimento.
pub(crate) fn setup_pick_store_cover(
    ui: &MainWindow,
    handle: &tokio::runtime::Handle,
) {
    let ui_weak = ui.as_weak();
    let handle = handle.clone();

    ui.global::<SettingsState>().on_pick_store_cover(move || {
        let ui_weak = ui_weak.clone();
        handle.spawn_blocking(move || {
            let Some(path) = pick_image_file() else { return };
            let uw = ui_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = uw.upgrade() { ui.global::<SettingsState>().set_store_cover_loading(true); }
            });
            match process_image_file_large(&path) {
                Some(encoded) => {
                    let pixel_buf = decode_pixel_buffer(&encoded);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak.upgrade() {
                            ui.global::<SettingsState>().set_store_cover_image(pixel_buf.map(slint::Image::from_rgba8).unwrap_or_default());
                            ui.global::<SettingsState>().set_store_cover_data(SharedString::from(encoded));
                            ui.global::<SettingsState>().set_store_cover_loading(false);
                        }
                    });
                }
                None => {
                    tracing::error!("Failed to process cover: {}", path.display());
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak.upgrade() { ui.global::<SettingsState>().set_store_cover_loading(false); }
                    });
                }
            }
        });
    });
}
