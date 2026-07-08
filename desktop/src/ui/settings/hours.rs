use std::sync::Arc;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use tokio::sync::Notify;

use letaf_core::business_hours::model::BusinessHours;

use crate::context::DesktopState;
use crate::format::format_phone;
use crate::{BusinessHoursData, MainWindow};

use super::super::helpers::show_toast;
use super::super::image::decode_pixel_buffer;

const DAY_NAMES: [&str; 7] = [
    "Domingo",
    "Segunda",
    "Terça",
    "Quarta",
    "Quinta",
    "Sexta",
    "Sábado",
];

/// Constrói os 7 dias da semana mesclando dados salvos com defaults.
pub(crate) fn build_all_days(saved: &[BusinessHours]) -> Vec<BusinessHoursData> {
    (0..7)
        .map(|day| {
            match saved.iter().find(|bh| bh.day_of_week == day) {
                Some(bh) => to_business_hours_data(bh),
                None => BusinessHoursData {
                    id: SharedString::default(),
                    day_of_week: day,
                    day_name: SharedString::from(DAY_NAMES[day as usize]),
                    open_time: SharedString::from("08:00"),
                    close_time: SharedString::from("18:00"),
                    is_open: false,
                },
            }
        })
        .collect()
}

pub(crate) fn to_business_hours_data(bh: &BusinessHours) -> BusinessHoursData {
    BusinessHoursData {
        id: SharedString::from(bh.base.id.to_string()),
        day_of_week: bh.day_of_week,
        day_name: SharedString::from(DAY_NAMES[bh.day_of_week.clamp(0, 6) as usize]),
        open_time: SharedString::from(&bh.open_time),
        close_time: SharedString::from(&bh.close_time),
        is_open: bh.is_open,
    }
}

/// Callback: carrega horários do SQLite e preenche os 7 dias.
///
/// Regras aplicadas (AI_RULES.md §1, §7, §8):
/// - Dias sem registro exibem defaults (offline-first)
pub(crate) fn setup_refresh_business_hours(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_refresh_business_hours(move || {
        let ui_weak = ui_weak.clone();
        let state = state.clone();

        handle.spawn(async move {
            let cid = state.company_id();
            let company = state.company_service.find_by_id(cid).await.ok().flatten();
            let opt = |getter: fn(&letaf_core::company::model::Company) -> Option<String>| {
                company.as_ref().and_then(getter).unwrap_or_default()
            };
            let override_status = company.as_ref().map(|c| c.store_override.clone()).unwrap_or_else(|| "none".to_string());
            let s_synced    = company.as_ref().map(|c| c.synced).unwrap_or(true);
            let s_name      = company.as_ref().map(|c| c.name.clone()).unwrap_or_default();
            let s_address   = opt(|c| c.address.clone());
            let s_phone     = opt(|c| c.phone.clone());
            let s_whatsapp  = opt(|c| c.whatsapp.clone());
            let s_email     = opt(|c| c.email.clone());
            let s_instagram = opt(|c| c.instagram.clone());
            let s_document  = opt(|c| c.document.clone());
            let s_neighbor  = opt(|c| c.neighborhood.clone());
            let s_zip       = opt(|c| c.zip_code.clone());
            let s_city      = opt(|c| c.city.clone());
            let s_uf        = opt(|c| c.uf.clone());
            let s_logo      = opt(|c| c.logo_data.clone());
            let s_cover     = opt(|c| c.cover_data.clone());
            let s_per_page    = company.as_ref().map(|c| c.products_per_page).unwrap_or(20);
            let s_orders_per_page = company.as_ref().map(|c| c.orders_per_page).unwrap_or(20);
            let logo_buf  = if s_logo.is_empty()  { None } else { decode_pixel_buffer(&s_logo) };
            let cover_buf = if s_cover.is_empty() { None } else { decode_pixel_buffer(&s_cover) };
            match state.business_hours_service.find_all(cid).await {
                Ok(saved) => {
                    let data = build_all_days(&saved);
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        ui.set_store_override(SharedString::from(override_status));
                        ui.set_store_name(SharedString::from(s_name));
                        ui.set_store_address(SharedString::from(s_address));
                        ui.set_store_phone(SharedString::from(format_phone(&s_phone)));
                        ui.set_store_whatsapp(SharedString::from(format_phone(&s_whatsapp)));
                        ui.set_store_email(SharedString::from(s_email));
                        ui.set_store_instagram(SharedString::from(s_instagram));
                        ui.set_store_document(SharedString::from(crate::format::format_document(&s_document)));
                        ui.set_store_neighborhood(SharedString::from(s_neighbor));
                        ui.set_store_zip_code(SharedString::from(s_zip));
                        ui.set_store_city(SharedString::from(s_city));
                        ui.set_store_uf(SharedString::from(s_uf));
                        ui.set_store_logo_data(SharedString::from(s_logo));
                        ui.set_store_logo_image(logo_buf.map(slint::Image::from_rgba8).unwrap_or_default());
                        ui.set_store_cover_data(SharedString::from(s_cover));
                        ui.set_store_cover_image(cover_buf.map(slint::Image::from_rgba8).unwrap_or_default());
                        ui.set_store_synced(s_synced);
                        ui.set_products_per_page(s_per_page);
                        ui.set_orders_per_page(s_orders_per_page);
                        ui.set_business_hours(ModelRc::new(VecModel::from(data)));
                    });
                }
                Err(e) => {
                    let msg = format!("Erro ao carregar horários: {e}");
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        ui.set_status_message(SharedString::from(msg));
                    });
                }
            }
        });
    });
}

/// Callback: aplica máscara HH:MM ao texto digitado pelo usuário.
///
/// Regras aplicadas (AI_RULES.md §1, §8):
/// - Lógica de formatação centralizada no Rust, não na UI (§1)
/// - Função pura: recebe string bruta, retorna string formatada (§8)
///
/// Comportamento:
/// - Filtra apenas dígitos '0'-'9'
/// - Aceita no máximo 4 dígitos
/// - Insere ':' automaticamente entre posições 2 e 3
/// - Clicar no campo seleciona tudo (via Slint), permitindo substituição total
pub(crate) fn setup_apply_time_mask(ui: &MainWindow) {
    ui.on_apply_time_mask(|s| {
        let digits: String = s.chars().filter(|c| c.is_ascii_digit()).take(4).collect();
        let formatted = match digits.len() {
            0..=2 => digits,
            _ => format!("{}:{}", &digits[..2], &digits[2..]),
        };
        SharedString::from(formatted)
    });
}

/// Callback: define override de status do estabelecimento ("none", "open", "closed").
///
/// Regras aplicadas (AI_RULES.md §1, §7, §8):
/// - Persiste no SQLite via service, dispara sync (§7.3)
pub(crate) fn setup_set_store_override(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_set_store_override(move |override_status| {
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let notify = sync_notify.clone();
        let override_str = override_status.to_string();

        handle.spawn(async move {
            let cid = state.company_id();
            match state.company_service.set_store_override(cid, override_str.clone()).await {
                Ok(_) => {
                    notify.notify_one();
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        ui.set_store_override(SharedString::from(override_str));
                        let label = match ui.get_store_override().as_str() {
                            "open" => "Estabelecimento Forçado: Aberto",
                            "closed" => "Estabelecimento Forçado: Fechado",
                            _ => "Desativado",
                        };
                        show_toast(&ui, label, "success");
                        ui.set_status_message(SharedString::from(label));
                    });
                }
                Err(e) => {
                    let msg = format!("Erro ao salvar override: {e}");
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

/// Callback: salva horário de um dia e dispara sync.
///
/// Regras aplicadas (AI_RULES.md §1, §7, §8):
/// - Salva no SQLite primeiro, notifica sync worker (§7.3)
pub(crate) fn setup_save_business_hours(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_save_business_hours(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };

        let day = ui_ref.get_bh_editing_day();
        let open_time = ui_ref.get_bh_open_time().to_string();
        let close_time = ui_ref.get_bh_close_time().to_string();
        let is_open = ui_ref.get_bh_is_open();

        let ui_weak = ui_ref.as_weak();
        let state = state.clone();
        let notify = sync_notify.clone();

        handle.spawn(async move {
            let cid = state.company_id();
            match state.business_hours_service.upsert(cid, day, open_time, close_time, is_open).await {
                Ok(_) => {
                    notify.notify_one();
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        show_toast(&ui, "Horário Salvo", "success");
                        ui.set_show_modal(false);
                        ui.set_status_message(SharedString::from("Horário Salvo"));
                        ui.invoke_refresh_business_hours();
                    });
                }
                Err(e) => {
                    let msg = format!("Erro ao salvar horário: {e}");
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

