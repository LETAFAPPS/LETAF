use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use uuid::Uuid;


use crate::context::DesktopState;
use crate::MainWindow;

use super::render::{apply_cache, apply_detail, build_tree, CatCache, resolve_category};

/// Registra os callbacks do master-detail de Categorias.
///
/// `on_refresh_categories` recarrega do SQLite (categorias +
/// subcategorias + produtos), preservando expansão/seleção — assim
/// add/update/delete/reorder (que chamam `invoke_refresh_categories`)
/// reconstroem a tela sem perder o contexto.
pub(crate) fn setup_categories(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
) {
    let cache: Arc<Mutex<CatCache>> = Arc::new(Mutex::new(CatCache::default()));

    // ── refresh ──
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        let handle = handle.clone();
        let cache = cache.clone();
        ui.on_refresh_categories(move || {
            let ui_weak = ui_weak.clone();
            let state = state.clone();
            let cache = cache.clone();
            handle.spawn(async move {
                let cid = state.company_id();
                let cats = state.category_service.find_all(cid).await;
                let subs = state.subcategory_service.find_all(cid).await;
                let prods = state.product_service.find_all(cid).await;
                match (cats, subs, prods) {
                    (Ok(cats), Ok(subs), Ok(prods)) => {
                        let _ = slint::invoke_from_event_loop(move || {
                            let Some(ui) = ui_weak.upgrade() else { return };
                            if let Ok(mut g) = cache.lock() {
                                g.categories = cats;
                                g.subcategories = subs;
                                g.products = prods;
                                let ids: HashSet<Uuid> =
                                    g.categories.iter().map(|c| c.base.id).collect();
                                g.expanded.retain(|id| ids.contains(id));
                                if let Some(sel) = g.selected {
                                    if resolve_category(&g, sel).is_none() {
                                        g.selected = None;
                                    }
                                }
                                apply_cache(&ui, &g);
                            }
                        });
                    }
                    (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => {
                        let _ = slint::invoke_from_event_loop(move || {
                            let Some(ui) = ui_weak.upgrade() else { return };
                            ui.set_status_message(SharedString::from(format!("Erro: {e}")));
                        });
                    }
                }
            });
        });
    }

    // ── seleção ──
    {
        let ui_weak = ui.as_weak();
        let cache = cache.clone();
        ui.on_select_category(move |id_str| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let Ok(id) = Uuid::parse_str(id_str.as_str()) else { return };
            if let Ok(mut g) = cache.lock() {
                g.selected = Some(id);
                apply_detail(&ui, &g);
            }
        });
    }

    // ── expandir/recolher ──
    {
        let ui_weak = ui.as_weak();
        let cache = cache.clone();
        ui.on_toggle_category(move |id_str| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let Ok(id) = Uuid::parse_str(id_str.as_str()) else { return };
            if let Ok(mut g) = cache.lock() {
                if g.expanded.contains(&id) {
                    g.expanded.remove(&id);
                } else {
                    g.expanded.insert(id);
                }
                let tree = build_tree(&g);
                ui.set_category_tree(ModelRc::new(VecModel::from(tree)));
            }
        });
    }
}

