//! Formulário de estabelecimento: preenchimento, limpeza, seletores de
//! imagem e máscaras dos campos.

use slint::{ComponentHandle, Model, SharedString};

use crate::{AdminState, MainWindow};

use super::super::image::{
    decode_pixel_buffer, pick_image_file, process_image_file, process_image_file_large,
};
use super::dto::CompanyFormDto;

/// Preenche o formulário com os dados de uma empresa e entra em modo edição.
pub(super) fn fill_company_form(ui: &MainWindow, f: &CompanyFormDto) {
    fill_identity(ui, f);
    fill_plan(ui, f);
    fill_images(ui, f);
    fill_owner(ui, f);
    // Entra em modo edição, formulário limpo de erros.
    let g = ui.global::<AdminState>();
    g.set_company_editing(true);
    g.set_company_edit_id(f.id.clone().into());
    g.set_company_form_attempted(false);
    g.set_company_show_form(true);
}

/// Identificação, contato e endereço.
fn fill_identity(ui: &MainWindow, f: &CompanyFormDto) {
    let g = ui.global::<AdminState>();
    g.set_company_form_name(f.name.clone().into());
    g.set_company_form_subdomain(f.subdomain.clone().into());
    g.set_company_form_document(f.document.clone().into());
    g.set_company_form_phone(f.phone.clone().into());
    g.set_company_form_whatsapp(f.whatsapp.clone().into());
    g.set_company_form_email(f.email.clone().into());
    g.set_company_form_address(f.address.clone().into());
    g.set_company_form_neighborhood(f.neighborhood.clone().into());
    g.set_company_form_zip(f.zip_code.clone().into());
    g.set_company_form_city(f.city.clone().into());
    g.set_company_form_uf(f.uf.clone().into());
    // Coordenadas: número → texto; None fica vazio.
    g.set_company_form_latitude(f.latitude.map(|v| v.to_string()).unwrap_or_default().into());
    g.set_company_form_longitude(f.longitude.map(|v| v.to_string()).unwrap_or_default().into());
}

/// Plano, período grátis e desconto.
fn fill_plan(ui: &MainWindow, f: &CompanyFormDto) {
    let g = ui.global::<AdminState>();
    // Plano atual (id do catálogo). Se vazio, cai no 1º plano ativo.
    let plan = if f.plan.is_empty() {
        let opts = g.get_company_form_plan_options();
        opts.row_data(0).map(|o| o.key.to_string()).unwrap_or_default()
    } else {
        f.plan.clone()
    };
    g.set_company_form_plan(plan.into());
    // Período grátis (dias): 0 fica vazio (mostra o placeholder).
    g.set_company_form_trial(if f.trial_days > 0 { f.trial_days.to_string() } else { String::new() }.into());
    // Desconto (R$/mês) em pt-BR; 0 fica vazio (mostra o placeholder).
    let discount = if f.discount > 0.0 {
        format!("{:.2}", f.discount).replace('.', ",")
    } else {
        String::new()
    };
    g.set_company_form_discount(discount.into());
}

/// Imagens (logo/capa) — decodifica o base64 para preview.
fn fill_images(ui: &MainWindow, f: &CompanyFormDto) {
    let g = ui.global::<AdminState>();
    g.set_company_form_logo_data(f.logo_data.clone().into());
    g.set_company_form_logo_image(
        decode_pixel_buffer(&f.logo_data)
            .map(slint::Image::from_rgba8)
            .unwrap_or_default(),
    );
    g.set_company_form_cover_data(f.cover_data.clone().into());
    g.set_company_form_cover_image(
        decode_pixel_buffer(&f.cover_data)
            .map(slint::Image::from_rgba8)
            .unwrap_or_default(),
    );
}

/// Proprietário (admin inicial): pré-preenchido e editável. A senha fica
/// em branco (vazio = mantém a atual).
fn fill_owner(ui: &MainWindow, f: &CompanyFormDto) {
    let g = ui.global::<AdminState>();
    g.set_company_form_admin_name(f.owner_name.clone().into());
    g.set_company_form_admin_email(f.owner_email.clone().into());
    g.set_company_form_admin_phone(f.owner_phone.clone().into());
    g.set_company_form_admin_password(SharedString::new());
}

/// Limpa todos os campos do formulário de novo estabelecimento.
pub(super) fn clear_company_form(ui: &MainWindow) {
    ui.global::<AdminState>().set_company_form_name(SharedString::new());
    ui.global::<AdminState>().set_company_form_subdomain(SharedString::new());
    ui.global::<AdminState>().set_company_form_admin_name(SharedString::new());
    ui.global::<AdminState>().set_company_form_admin_email(SharedString::new());
    ui.global::<AdminState>().set_company_form_admin_password(SharedString::new());
    ui.global::<AdminState>().set_company_form_admin_phone(SharedString::new());
    ui.global::<AdminState>().set_company_form_phone(SharedString::new());
    ui.global::<AdminState>().set_company_form_whatsapp(SharedString::new());
    ui.global::<AdminState>().set_company_form_email(SharedString::new());
    ui.global::<AdminState>().set_company_form_document(SharedString::new());
    ui.global::<AdminState>().set_company_form_discount(SharedString::new());
    ui.global::<AdminState>().set_company_form_address(SharedString::new());
    ui.global::<AdminState>().set_company_form_neighborhood(SharedString::new());
    ui.global::<AdminState>().set_company_form_zip(SharedString::new());
    ui.global::<AdminState>().set_company_form_city(SharedString::new());
    ui.global::<AdminState>().set_company_form_uf(SharedString::new());
    ui.global::<AdminState>().set_company_form_latitude(SharedString::new());
    ui.global::<AdminState>().set_company_form_longitude(SharedString::new());
    // Plano: 1º plano ativo disponível (senão mensal).
    let default_plan = {
        let opts = ui.global::<AdminState>().get_company_form_plan_options();
        opts.row_data(0).map(|o| o.key.to_string()).unwrap_or_else(|| "monthly".into())
    };
    ui.global::<AdminState>().set_company_form_plan(default_plan.into());
    ui.global::<AdminState>().set_company_form_trial(SharedString::new());
    ui.global::<AdminState>().set_company_form_logo_data(SharedString::new());
    ui.global::<AdminState>().set_company_form_cover_data(SharedString::new());
    ui.global::<AdminState>().set_company_form_logo_image(slint::Image::default());
    ui.global::<AdminState>().set_company_form_cover_image(slint::Image::default());
}

/// Seletores de logo/capa do novo estabelecimento (espelha Configurações).
pub(super) fn setup_company_pickers(ui: &MainWindow, handle: &tokio::runtime::Handle) {
    setup_logo_picker(ui, handle);
    setup_cover_picker(ui, handle);
}

/// Logo (imagem menor).
fn setup_logo_picker(ui: &MainWindow, handle: &tokio::runtime::Handle) {
    let ui_weak = ui.as_weak();
    let handle = handle.clone();
    ui.global::<AdminState>().on_pick_company_logo(move || {
        let ui_weak = ui_weak.clone();
        handle.spawn_blocking(move || {
            let Some(path) = pick_image_file() else { return };
            let uw = ui_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = uw.upgrade() { ui.global::<AdminState>().set_company_form_logo_loading(true); }
            });
            let encoded = process_image_file(&path);
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                ui.global::<AdminState>().set_company_form_logo_loading(false);
                if let Some(enc) = encoded {
                    let buf = decode_pixel_buffer(&enc);
                    ui.global::<AdminState>().set_company_form_logo_image(buf.map(slint::Image::from_rgba8).unwrap_or_default());
                    ui.global::<AdminState>().set_company_form_logo_data(SharedString::from(enc));
                }
            });
        });
    });
}

/// Capa (imagem maior).
fn setup_cover_picker(ui: &MainWindow, handle: &tokio::runtime::Handle) {
    let ui_weak = ui.as_weak();
    let handle = handle.clone();
    ui.global::<AdminState>().on_pick_company_cover(move || {
        let ui_weak = ui_weak.clone();
        handle.spawn_blocking(move || {
            let Some(path) = pick_image_file() else { return };
            let uw = ui_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = uw.upgrade() { ui.global::<AdminState>().set_company_form_cover_loading(true); }
            });
            let encoded = process_image_file_large(&path);
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                ui.global::<AdminState>().set_company_form_cover_loading(false);
                if let Some(enc) = encoded {
                    let buf = decode_pixel_buffer(&enc);
                    ui.global::<AdminState>().set_company_form_cover_image(buf.map(slint::Image::from_rgba8).unwrap_or_default());
                    ui.global::<AdminState>().set_company_form_cover_data(SharedString::from(enc));
                }
            });
        });
    });
}

/// Máscara e verificação dos campos do cadastro (feedback de UX; §11).
/// Puras — o Slint as chama em expressões de propriedade.
pub(super) fn setup_company_form_helpers(ui: &MainWindow) {
    use crate::format::{field_error, format_document, format_money_input, format_phone,
        format_zip_code, sanitize_subdomain};
    ui.global::<AdminState>().on_apply_mask(|kind, value| {
        let out = match kind.as_str() {
            "document" => format_document(&value),
            "phone" => format_phone(&value),
            "cep" => format_zip_code(&value),
            "money" => format_money_input(&value),
            "subdomain" => sanitize_subdomain(&value),
            "digits" => value.chars().filter(|c| c.is_ascii_digit()).collect(),
            _ => value.to_string(),
        };
        SharedString::from(out)
    });
    ui.global::<AdminState>().on_field_error(|rule, value| {
        SharedString::from(field_error(&rule, &value))
    });
}
