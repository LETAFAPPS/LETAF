use std::collections::HashSet;
use std::sync::Arc;

use slint::{ComponentHandle, Model, SharedString, VecModel};
use tokio::sync::Notify;
use uuid::Uuid;


use crate::context::DesktopState;
use crate::{MainWindow, ProductData};

use super::super::helpers::show_toast;
use super::super::image::{
    decode_pixel_buffer, pick_image_file, process_product_image,
};
use super::state::DecodedProduct;
use super::data::{parse_hex_color, update_detail_product_flag, update_product_flag};

/// Listener leve do worker — atualiza apenas o flag `synced` (e o
/// `sync-label`) na lista e no `detail-product` quando o worker fecha
/// um ciclo. NÃO re-decodifica imagens (refresh-products faria isso e
/// disparar a cada ciclo de sync, com catálogo grande, fica caro).
///
/// Regras aplicadas (AI_RULES.md §1, §3, §7):
/// - O Rust traz a verdade do banco (find_unsynced); a UI só pinta.
/// - Mutação cirúrgica do `VecModel` (`remove + insert`) força
///   re-render dos `if` condicionais que dependem de `synced` (mesmo
///   padrão de `update_product_flag`).
pub(crate) fn setup_sync_listener(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    cache: Arc<std::sync::Mutex<Vec<DecodedProduct>>>,
    cycle_done: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    handle.spawn(async move {
        loop {
            cycle_done.notified().await;
            // Lê IDs pendentes do banco (cheap — sem decodificar imagens).
            let cid = state.company_id();
            let pending_ids: HashSet<String> = match state.product_service.find_unsynced(cid).await {
                Ok(v) => v.iter().map(|p| p.base.id.to_string()).collect(),
                Err(e) => {
                    tracing::warn!("setup_sync_listener: find_unsynced falhou: {e}");
                    continue;
                }
            };
            // Atualiza cache (Send) e UI no event loop.
            if let Ok(mut g) = cache.lock() {
                for p in g.iter_mut() {
                    let is_synced = !pending_ids.contains(p.id.as_str());
                    if p.synced != is_synced {
                        p.synced = is_synced;
                        p.sync_label = SharedString::from(
                            if is_synced { "Sincronizado" } else { "Aguardando Sincronização" }
                        );
                    }
                }
            }
            let pending_ids_arc = Arc::new(pending_ids);
            let ui_weak2 = ui_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak2.upgrade() else { return };
                // 1) Lista lateral: atualiza cada linha cujo `synced` mudou.
                let model = ui.get_products();
                if let Some(vm) = model.as_any().downcast_ref::<VecModel<ProductData>>() {
                    for i in 0..vm.row_count() {
                        if let Some(mut p) = vm.row_data(i) {
                            let is_synced = !pending_ids_arc.contains(p.id.as_str());
                            if p.synced != is_synced {
                                p.synced = is_synced;
                                p.sync_label = SharedString::from(
                                    if is_synced { "Sincronizado" } else { "Aguardando Sincronização" }
                                );
                                vm.remove(i);
                                vm.insert(i, p);
                            }
                        }
                    }
                }
                // 2) Painel direito: detail-product.
                let mut detail = ui.get_detail_product();
                if !detail.id.is_empty() {
                    let is_synced = !pending_ids_arc.contains(detail.id.as_str());
                    if detail.synced != is_synced {
                        detail.synced = is_synced;
                        detail.sync_label = SharedString::from(
                            if is_synced { "Sincronizado" } else { "Aguardando Sincronização" }
                        );
                        ui.set_detail_product(detail);
                    }
                }
            });
        }
    });
}

/// Remove um produto do modelo pelo ID sem reload completo.
///
/// Regras aplicadas (AI_RULES.md §8, §13): atualização cirúrgica — sem re-decode.
pub(crate) fn remove_product_from_model(ui: &MainWindow, id: &SharedString) {
    let model = ui.get_products();
    if let Some(vm) = model.as_any().downcast_ref::<VecModel<ProductData>>() {
        for i in 0..vm.row_count() {
            if vm.row_data(i).map(|p| p.id == id).unwrap_or(false) {
                vm.remove(i);
                return;
            }
        }
    }
}

/// Callback: abre seletor, redimensiona e converte imagem para JPEG base64.
///
/// Regras aplicadas (AI_RULES.md §3, §8):
/// - UI delega ao Rust; sem lógica de negócio no .slint
/// - Orquestra apenas; processamento isolado em funções auxiliares (§8)
pub(crate) fn setup_pick_product_image(
    ui: &MainWindow,
    handle: &tokio::runtime::Handle,
    cache: Arc<std::sync::Mutex<Vec<DecodedProduct>>>,
) {
    let ui_weak = ui.as_weak();
    let handle = handle.clone();

    ui.on_pick_product_image(move || {
        let ui_weak = ui_weak.clone();
        let cache = cache.clone();
        handle.spawn_blocking(move || {
            let Some(path) = pick_image_file() else { return };

            // Arquivo selecionado — sinaliza início do processamento na UI
            let uw = ui_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = uw.upgrade() { ui.set_product_image_loading(true); }
            });

            match process_product_image(&path) {
                Some((encoded, cover_color)) => {
                    let cover = cover_color.unwrap_or_default();
                    // Decodifica o base64 → SharedPixelBuffer ANTES de
                    // entrar no event loop (operação CPU-bound). Assim o
                    // quadrado da imagem no header do master-detail
                    // atualiza junto com `product-image-data`, sem
                    // esperar refresh-products.
                    let pixel_buf = decode_pixel_buffer(&encoded);
                    let cover_rgb = parse_hex_color(&cover);
                    let cache_for_loop = cache.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak.upgrade() {
                            ui.set_product_image_data(SharedString::from(encoded));
                            ui.set_product_cover_color(SharedString::from(cover));
                            ui.set_product_image_loading(false);
                            // Atualiza o snapshot do painel direito —
                            // `detail-product.product-image` é uma
                            // `slint::Image`; gravar aqui faz o header
                            // refletir em tempo real.
                            let mut detail = ui.get_detail_product();
                            detail.image_data = ui.get_product_image_data();
                            detail.cover_color = ui.get_product_cover_color();
                            if let Some((r, g, b)) = cover_rgb {
                                detail.cover_color_value = slint::Color::from_rgb_u8(r, g, b);
                                detail.has_cover_color = true;
                            } else {
                                detail.cover_color_value = slint::Color::default();
                                detail.has_cover_color = false;
                            }
                            detail.product_image = pixel_buf
                                .clone()
                                .map(slint::Image::from_rgba8)
                                .unwrap_or_default();
                            ui.set_detail_product(detail.clone());
                            // Atualiza também a linha da lista mestra
                            // (não só o detalhe). Sem isso, a miniatura
                            // à esquerda só refletiria a nova imagem
                            // após o save → a tela parecia "presa" na
                            // imagem antiga.
                            let editing_id = ui.get_editing_id();
                            if !editing_id.is_empty() {
                                update_product_flag(&ui, &editing_id, |p| {
                                    p.image_data = detail.image_data.clone();
                                    p.cover_color = detail.cover_color.clone();
                                    p.cover_color_value = detail.cover_color_value;
                                    p.has_cover_color = detail.has_cover_color;
                                    p.product_image = detail.product_image.clone();
                                });
                                // Atualiza o cache também — sem isso,
                                // `setup_select_product` reverteria à
                                // imagem antiga quando o operador
                                // saísse e voltasse no produto.
                                if let Ok(mut g) = cache_for_loop.lock() {
                                    if let Some(d) = g.iter_mut().find(|d| d.id == editing_id) {
                                        d.image_data = detail.image_data.clone();
                                        d.cover_color = detail.cover_color.clone();
                                        d.cover_color_rgb = cover_rgb;
                                        d.pixel_buffer = pixel_buf.clone();
                                    }
                                }
                            }
                        }
                    });
                }
                None => {
                    tracing::error!("Failed to process image: {}", path.display());
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak.upgrade() { ui.set_product_image_loading(false); }
                    });
                }
            }
        });
    });
}

/// Callback: alterna ativo/inativo de um produto.
///
/// Regras aplicadas (AI_RULES.md §1, §3, §8):
/// - UI delega ao service; sem lógica de negócio aqui
/// - Estado atual lido do modelo Slint para determinar o novo valor
pub(crate) fn setup_toggle_product_active(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
    cache: Arc<std::sync::Mutex<Vec<DecodedProduct>>>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_toggle_product_active(move |id_str| {
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        let Ok(id) = Uuid::parse_str(id_str.as_str()) else { return };

        let new_active = !ui_ref
            .get_products()
            .iter()
            .find(|p| p.id == id_str)
            .map(|p| p.active)
            .unwrap_or(true);

        let id_ss = id_str.clone();
        let ui_weak = ui_ref.as_weak();
        let state = state.clone();
        let notify = sync_notify.clone();
        let cache = cache.clone();

        handle.spawn(async move {
            let cid = state.company_id();
            match state.product_service.toggle_active(cid, id, new_active).await {
                Ok(()) => {
                    notify.notify_one();
                    // Atualiza cache: active + synced=false (toggle dispara
                    // re-sync). Sem isso, voltar no produto repõe o estado
                    // anterior pelo `setup_select_product`.
                    if let Ok(mut g) = cache.lock() {
                        if let Some(d) = g.iter_mut().find(|d| d.id == id_ss) {
                            d.active = new_active;
                            d.synced = false;
                            d.sync_label = SharedString::from("Aguardando Sincronização");
                        }
                    }
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        let label = if new_active { "Ativado" } else { "Desativado" };
                        show_toast(&ui, &format!("Produto {label}"), "success");
                        ui.set_status_message(SharedString::from(format!("Produto {label}")));
                        update_product_flag(&ui, &id_ss, |p| { p.active = new_active; });
                        update_detail_product_flag(&ui, &id_ss, |p| { p.active = new_active; });
                        update_active_counts(&ui);
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

/// Recalcula contadores de produtos ativos/inativos a partir do model atual.
///
/// Chamado após cada toggle de `active` — mantém o subtítulo da página em
/// sincronia sem recarregar a lista.
fn update_active_counts(ui: &MainWindow) {
    let model = ui.get_products();
    let mut active = 0i32;
    let mut inactive = 0i32;
    for i in 0..model.row_count() {
        if let Some(p) = model.row_data(i) {
            if p.active { active += 1; } else { inactive += 1; }
        }
    }
    ui.set_products_active_count(active);
    ui.set_products_inactive_count(inactive);
}

/// Callback: alterna visibilidade do produto no cardápio web.
///
/// Regras aplicadas (AI_RULES.md §1, §3, §8, §11):
/// - Independente do estado `active` (que controla PDV + web simultaneamente).
/// - Quando `active=true` e `web_visible=false`, o produto continua disponível
///   no PDV (Fase 2) mas é oculto na web (cardápio cliente).
pub(crate) fn setup_toggle_web_visible(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
    cache: Arc<std::sync::Mutex<Vec<DecodedProduct>>>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_toggle_product_web_visible(move |id_str| {
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        let Ok(id) = Uuid::parse_str(id_str.as_str()) else { return };

        let new_visible = !ui_ref
            .get_products()
            .iter()
            .find(|p| p.id == id_str)
            .map(|p| p.web_visible)
            .unwrap_or(true);

        let id_ss = id_str.clone();
        let ui_weak = ui_ref.as_weak();
        let state = state.clone();
        let notify = sync_notify.clone();
        let cache = cache.clone();

        handle.spawn(async move {
            let cid = state.company_id();
            match state.product_service.toggle_web_visible(cid, id, new_visible).await {
                Ok(()) => {
                    notify.notify_one();
                    if let Ok(mut g) = cache.lock() {
                        if let Some(d) = g.iter_mut().find(|d| d.id == id_ss) {
                            d.web_visible = new_visible;
                            d.synced = false;
                            d.sync_label = SharedString::from("Aguardando Sincronização");
                        }
                    }
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        let label = if new_visible { "Visível na Web" } else { "Oculto na Web" };
                        show_toast(&ui, &format!("Produto {label}"), "success");
                        ui.set_status_message(SharedString::from(format!("Produto {label}")));
                        update_product_flag(&ui, &id_ss, |p| { p.web_visible = new_visible; });
                        update_detail_product_flag(&ui, &id_ss, |p| { p.web_visible = new_visible; });
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
