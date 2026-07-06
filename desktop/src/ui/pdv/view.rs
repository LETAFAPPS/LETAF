use std::sync::{Arc, Mutex};

use slint::{Image, ModelRc, SharedString, VecModel};


use crate::{MainWindow, PdvCartRow, PdvCategoryChip, PdvProductRow};

use super::state::{fmt_brl, PdvState};

// ── Renderização ──────────────────────────────────────────────

pub(crate) fn apply_state_to_ui(ui: &MainWindow, pdv: &Arc<Mutex<PdvState>>) {
    let Ok(g) = pdv.lock() else { return };
    let chips: Vec<PdvCategoryChip> = g.categories.iter().map(|(id, name)| {
        let selected = g.active_category_ids.contains(id);
        PdvCategoryChip {
            id: SharedString::from(id.to_string()),
            name: SharedString::from(name.clone()),
            selected,
        }
    }).collect();
    // Sempre em uma única linha (sem split). Em telas menores os
    // chips comprimem via `min-width` no Slint; se o conteúdo
    // estourar, o ScrollView horizontal do bloco de categorias
    // permite scroll lateral.
    ui.set_pdv_categories(ModelRc::new(VecModel::from(chips)));
    ui.set_pdv_categories_row2(ModelRc::new(VecModel::from(Vec::<PdvCategoryChip>::new())));

    let q_lower = g.search_query.to_lowercase();
    let products: Vec<PdvProductRow> = g.products_all.iter()
        .filter(|p| p.active)
        .filter(|p| {
            if g.active_category_ids.is_empty() { return true; }
            p.category_id.is_some_and(|c| g.active_category_ids.contains(&c))
        })
        .filter(|p| {
            if q_lower.is_empty() { return true; }
            p.name.to_lowercase().contains(&q_lower)
                || p.barcode.as_deref().map(|b| b.contains(&q_lower)).unwrap_or(false)
        })
        .map(|p| {
            let has_variations = p.variations.as_deref()
                .map(|s| !s.trim().is_empty() && s.trim() != "[]")
                .unwrap_or(false);
            let has_addons = !p.addon_group_ids.is_empty();
            // Imagem do cache (Rc interno, clone barato).
            let (image, has_image) = match g.image_cache.get(&p.base.id) {
                Some(buf) => (Image::from_rgba8(buf.clone()), true),
                None => (Image::default(), false),
            };
            PdvProductRow {
                id: SharedString::from(p.base.id.to_string()),
                name: SharedString::from(p.name.clone()),
                price_display: SharedString::from(
                    p.price.map(|v| format!("R$ {:.2}", v)).unwrap_or_else(|| "".into())
                ),
                has_config: has_variations || has_addons,
                barcode: SharedString::from(p.barcode.clone().unwrap_or_default()),
                product_image: image,
                has_image,
            }
        })
        .collect();
    ui.set_pdv_products(ModelRc::new(VecModel::from(products)));

    let cart_rows: Vec<PdvCartRow> = g.cart.iter().map(|line| {
        let subtotal = (line.qty * line.unit_price).max(0.0);
        PdvCartRow {
            line_id: SharedString::from(line.line_id.to_string()),
            product_id: SharedString::from(line.product_id.to_string()),
            name: SharedString::from(line.name.clone()),
            qty: line.qty as i32,
            unit_price: line.unit_price as f32,
            addons_summary: SharedString::from(line.addons_summary.clone()),
            addons_json: SharedString::from(line.addons_json.clone().unwrap_or_default()),
            line_total_display: SharedString::from(fmt_brl(subtotal)),
        }
    }).collect();
    let subtotal: f64 = g.subtotal().max(0.0);
    let discount = g.discount_value.min(subtotal).max(0.0);
    let additional = g.additional_value.max(0.0);
    let total = (subtotal - discount + additional).max(0.0);
    let count: i32 = g.cart.iter().map(|l| l.qty as i32).sum();
    ui.set_pdv_cart(ModelRc::new(VecModel::from(cart_rows)));
    ui.set_pdv_subtotal_display(SharedString::from(fmt_brl(subtotal)));
    ui.set_pdv_discount_display(SharedString::from(fmt_brl(discount)));
    ui.set_pdv_additional_display(SharedString::from(fmt_brl(additional)));
    ui.set_pdv_total_display(SharedString::from(fmt_brl(total)));
    ui.set_pdv_total_amount(total as f32);
    ui.set_pdv_cart_count(count);

    // Troco / restante (só relevante em payment_method == "cash").
    let method = ui.get_pdv_payment_method().to_string();
    if method == "cash" {
        let paid = g.amount_paid;
        if paid >= total && total > 0.0 {
            let change = (paid - total).max(0.0);
            ui.set_pdv_change_display(SharedString::from(fmt_brl(change)));
            ui.set_pdv_remaining_display(SharedString::default());
        } else if paid > 0.0 && paid < total {
            let remaining = (total - paid).max(0.0);
            ui.set_pdv_remaining_display(SharedString::from(fmt_brl(remaining)));
            ui.set_pdv_change_display(SharedString::default());
        } else {
            ui.set_pdv_change_display(SharedString::default());
            ui.set_pdv_remaining_display(SharedString::default());
        }
    } else {
        ui.set_pdv_change_display(SharedString::default());
        ui.set_pdv_remaining_display(SharedString::default());
    }
}
