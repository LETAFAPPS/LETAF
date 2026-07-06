use std::collections::HashMap;
use rust_decimal::prelude::ToPrimitive;

use slint::{Model, SharedString, VecModel};
use uuid::Uuid;

use letaf_core::product::model::{Product, StockStatus};

use crate::format::{format_stock, money_br};
use crate::{MainWindow, ProductData};

use super::super::image::decode_pixel_buffer;
use super::state::DecodedProduct;

/// Conjunto de strings já formatadas para a UI a partir de um `Product`.
///
/// Regras aplicadas (AI_RULES.md §1, §3, §14):
/// - Toda a lógica de negócio (margem, status, sugestão de compra) vive
///   no core (`Product::margin_pct`, `stock_status`, `purchase_suggestion`).
///   Aqui só formatamos pt-BR para a apresentação.
/// - Fonte única usada por `to_decoded_product` (cache) e por
///   `build_product_data_from_product` (após create/update) — sem
///   risco de strings divergirem entre as duas rotas.
pub(crate) struct ProductDisplay {
    pub(crate) price: SharedString,
    pub(crate) price_display: SharedString,
    pub(crate) cost_price: SharedString,
    pub(crate) cost_price_display: SharedString,
    pub(crate) margin_amount_display: SharedString,
    pub(crate) margin_pct_display: SharedString,
    pub(crate) stock_quantity: SharedString,
    pub(crate) stock_status: SharedString,
    pub(crate) stock_status_label: SharedString,
    pub(crate) min_stock: SharedString,
    pub(crate) min_stock_display: SharedString,
    pub(crate) purchase_suggestion: SharedString,
    pub(crate) synced: bool,
    pub(crate) sync_label: SharedString,
}

// Padrão do sistema: sem dado para exibir → não mostra nada (string
// vazia), em vez de um placeholder "—".
const DASH: &str = "";

/// Texto curto do status para exibir no card/detalhe.
pub(crate) fn stock_status_label(s: StockStatus) -> &'static str {
    match s {
        StockStatus::Unlimited => "Ilimitado",
        StockStatus::Out       => "Esgotado",
        StockStatus::Low       => "Estoque Baixo",
        StockStatus::Ok        => "Em Estoque",
    }
}

/// Formata `min_stock` (ou `stock_quantity`) com a unidade pt-BR.
fn format_qty_with_unit(qty: f64, unit: &str) -> String {
    format!("{} {}", format_stock(qty, unit), unit)
}

/// Constrói o pacote de strings formatadas para a UI.
pub(crate) fn make_product_display(p: &Product) -> ProductDisplay {
    let price_raw = p.price.map(|v| format!("{:.2}", v.to_f64().unwrap_or(0.0))).unwrap_or_default();
    let price_display = match p.price {
        Some(v) => money_br(v),
        None => "Sem preço".to_string(),
    };
    let cost_raw = p.cost_price.map(|v| format!("{:.2}", v.to_f64().unwrap_or(0.0))).unwrap_or_default();
    let cost_display = match p.cost_price {
        Some(v) => money_br(v),
        None => DASH.to_string(),
    };
    let margin_amount_display = match p.margin_amount() {
        Some(v) => money_br(letaf_core::money::from_db_f64(v)),
        None => DASH.to_string(),
    };
    let margin_pct_display = match p.margin_pct() {
        Some(v) => format!("{:.1} %", v).replace('.', ","),
        None => DASH.to_string(),
    };
    let status = p.stock_status();
    let stock_status_slug = status.as_slug();
    let stock_status_lbl = stock_status_label(status);
    let stock_qty_str = if p.unlimited_stock {
        "∞".to_string()
    } else {
        format_stock(p.stock_quantity, &p.unit)
    };
    // Mínimo cru só existe quando configurado — vazio facilita o
    // placeholder do TextInput de edição.
    let min_stock_raw = if p.min_stock > 0.0 {
        format!("{:.3}", p.min_stock).trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        String::new()
    };
    let min_stock_display = if p.unlimited_stock || p.min_stock <= 0.0 {
        DASH.to_string()
    } else {
        format_qty_with_unit(p.min_stock, &p.unit)
    };
    let suggestion = p.purchase_suggestion();
    let purchase_suggestion = if suggestion > 0.0 {
        format!("Comprar {}", format_qty_with_unit(suggestion, &p.unit))
    } else {
        String::new()
    };
    let sync_label = if p.base.synced { "Sincronizado" } else { "Aguardando Sincronização" };
    ProductDisplay {
        price: SharedString::from(price_raw),
        price_display: SharedString::from(price_display),
        cost_price: SharedString::from(cost_raw),
        cost_price_display: SharedString::from(cost_display),
        margin_amount_display: SharedString::from(margin_amount_display),
        margin_pct_display: SharedString::from(margin_pct_display),
        stock_quantity: SharedString::from(stock_qty_str),
        stock_status: SharedString::from(stock_status_slug),
        stock_status_label: SharedString::from(stock_status_lbl),
        min_stock: SharedString::from(min_stock_raw),
        min_stock_display: SharedString::from(min_stock_display),
        purchase_suggestion: SharedString::from(purchase_suggestion),
        synced: p.base.synced,
        sync_label: SharedString::from(sync_label),
    }
}

/// Converte Product → DecodedProduct (roda em spawn_blocking).
///
/// Realiza o trabalho CPU-bound: base64 decode + decompress de imagem.
pub(crate) fn to_decoded_product(
    p: &Product,
    cat_map: &HashMap<String, String>,
    sub_map: &HashMap<String, String>,
) -> DecodedProduct {
    let cat_id = p.category_id.map(|u| u.to_string()).unwrap_or_default();
    let sub_id = p.subcategory_id.map(|u| u.to_string()).unwrap_or_default();
    let cover_color = p.cover_color.clone().unwrap_or_default();
    let cover_color_rgb = parse_hex_color(&cover_color);
    let disp = make_product_display(p);
    DecodedProduct {
        id: SharedString::from(p.base.id.to_string()),
        name: SharedString::from(p.name.as_str()),
        description: SharedString::from(p.description.as_deref().unwrap_or("")),
        price: disp.price,
        price_display: disp.price_display,
        cost_price: disp.cost_price,
        cost_price_display: disp.cost_price_display,
        margin_amount_display: disp.margin_amount_display,
        margin_pct_display: disp.margin_pct_display,
        stock_quantity: disp.stock_quantity,
        stock_status: disp.stock_status,
        stock_status_label: disp.stock_status_label,
        min_stock: disp.min_stock,
        min_stock_display: disp.min_stock_display,
        purchase_suggestion: disp.purchase_suggestion,
        unlimited_stock: p.unlimited_stock,
        barcode: SharedString::from(p.barcode.as_deref().unwrap_or("")),
        unit: SharedString::from(p.unit.as_str()),
        active: p.active,
        web_visible: p.web_visible,
        synced: disp.synced,
        sync_label: disp.sync_label,
        balance_mode: SharedString::from(p.balance_mode.as_db_str()),
        image_data: SharedString::from(p.image_data.as_deref().unwrap_or("")),
        category_name: SharedString::from(cat_map.get(&cat_id).map(|s| s.as_str()).unwrap_or("")),
        subcategory_name: SharedString::from(sub_map.get(&sub_id).map(|s| s.as_str()).unwrap_or("")),
        category_id: SharedString::from(cat_id),
        subcategory_id: SharedString::from(sub_id),
        cover_color: SharedString::from(cover_color),
        cover_color_rgb,
        availability_schedule: SharedString::from(p.availability_schedule.as_deref().unwrap_or("")),
        discount_kind: SharedString::from(p.discount_kind.as_deref().unwrap_or("")),
        discount_value: SharedString::from(
            p.discount_value.map(|v| format!("{v}")).unwrap_or_default()
        ),
        discount_min_qty: SharedString::from(
            p.discount_min_qty.map(|v| format!("{v}")).unwrap_or_default()
        ),
        discount_tiers: SharedString::from(p.discount_tiers.as_deref().unwrap_or("")),
        addon_group_ids: SharedString::from(addon_group_ids_to_csv(&p.addon_group_ids)),
        variations: SharedString::from(p.variations.as_deref().unwrap_or("")),
        pixel_buffer: p.image_data.as_deref()
            .filter(|s| !s.is_empty())
            .and_then(decode_pixel_buffer),
    }
}

/// Converte vetor de UUIDs em CSV — formato compacto que o Slint pode
/// passar como `string` para o callback `load-product-addon-groups`.
pub(crate) fn addon_group_ids_to_csv(ids: &[Uuid]) -> String {
    ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",")
}

pub(crate) fn parse_addon_group_ids_csv(csv: &str) -> Vec<Uuid> {
    csv.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter_map(|s| Uuid::parse_str(s).ok())
        .collect()
}

/// Parseia `#RRGGBB` → `(r, g, b)`. Retorna `None` em qualquer formato inválido.
pub(crate) fn parse_hex_color(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.strip_prefix('#')?;
    if s.len() != 6 { return None; }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some((r, g, b))
}

/// Converte &DecodedProduct → ProductData sem consumir (para filtros no event loop).
pub(crate) fn decoded_to_product_data_ref(d: &DecodedProduct) -> ProductData {
    let (cover_color_value, has_cover_color) = match d.cover_color_rgb {
        Some((r, g, b)) => (slint::Color::from_rgb_u8(r, g, b), true),
        None => (slint::Color::default(), false),
    };
    ProductData {
        id: d.id.clone(),
        name: d.name.clone(),
        description: d.description.clone(),
        price: d.price.clone(),
        price_display: d.price_display.clone(),
        cost_price: d.cost_price.clone(),
        cost_price_display: d.cost_price_display.clone(),
        margin_amount_display: d.margin_amount_display.clone(),
        margin_pct_display: d.margin_pct_display.clone(),
        stock_quantity: d.stock_quantity.clone(),
        min_stock: d.min_stock.clone(),
        min_stock_display: d.min_stock_display.clone(),
        purchase_suggestion: d.purchase_suggestion.clone(),
        stock_status: d.stock_status.clone(),
        stock_status_label: d.stock_status_label.clone(),
        unlimited_stock: d.unlimited_stock,
        barcode: d.barcode.clone(),
        unit: d.unit.clone(),
        active: d.active,
        web_visible: d.web_visible,
        synced: d.synced,
        sync_label: d.sync_label.clone(),
        balance_mode: d.balance_mode.clone(),
        image_data: d.image_data.clone(),
        category_id: d.category_id.clone(),
        category_name: d.category_name.clone(),
        subcategory_id: d.subcategory_id.clone(),
        subcategory_name: d.subcategory_name.clone(),
        cover_color: d.cover_color.clone(),
        cover_color_value,
        has_cover_color,
        availability_schedule: d.availability_schedule.clone(),
        discount_kind: d.discount_kind.clone(),
        discount_value: d.discount_value.clone(),
        discount_min_qty: d.discount_min_qty.clone(),
        discount_tiers: d.discount_tiers.clone(),
        addon_group_ids: d.addon_group_ids.clone(),
        variations: d.variations.clone(),
        product_image: d.pixel_buffer.clone()
            .map(slint::Image::from_rgba8)
            .unwrap_or_default(),
    }
}

/// Constrói ProductData a partir de Product + nomes pré-resolvidos + pixel buffer.
///
/// Regras aplicadas (AI_RULES.md §8): responsabilidade única — montagem de dados.
pub(crate) fn build_product_data_from_product(
    p: &Product,
    cat_name: &str,
    sub_name: &str,
    pixel_buf: Option<slint::SharedPixelBuffer<slint::Rgba8Pixel>>,
) -> ProductData {
    let cover_hex = p.cover_color.clone().unwrap_or_default();
    let (cover_color_value, has_cover_color) = match parse_hex_color(&cover_hex) {
        Some((r, g, b)) => (slint::Color::from_rgb_u8(r, g, b), true),
        None => (slint::Color::default(), false),
    };
    let disp = make_product_display(p);
    ProductData {
        id: SharedString::from(p.base.id.to_string()),
        name: SharedString::from(p.name.as_str()),
        description: SharedString::from(p.description.as_deref().unwrap_or("")),
        price: disp.price,
        price_display: disp.price_display,
        cost_price: disp.cost_price,
        cost_price_display: disp.cost_price_display,
        margin_amount_display: disp.margin_amount_display,
        margin_pct_display: disp.margin_pct_display,
        stock_quantity: disp.stock_quantity,
        min_stock: disp.min_stock,
        min_stock_display: disp.min_stock_display,
        purchase_suggestion: disp.purchase_suggestion,
        stock_status: disp.stock_status,
        stock_status_label: disp.stock_status_label,
        unlimited_stock: p.unlimited_stock,
        barcode: SharedString::from(p.barcode.as_deref().unwrap_or("")),
        unit: SharedString::from(p.unit.as_str()),
        active: p.active,
        web_visible: p.web_visible,
        synced: disp.synced,
        sync_label: disp.sync_label,
        balance_mode: SharedString::from(p.balance_mode.as_db_str()),
        image_data: SharedString::from(p.image_data.as_deref().unwrap_or("")),
        category_id: SharedString::from(p.category_id.map(|u| u.to_string()).unwrap_or_default()),
        category_name: SharedString::from(cat_name),
        subcategory_id: SharedString::from(p.subcategory_id.map(|u| u.to_string()).unwrap_or_default()),
        subcategory_name: SharedString::from(sub_name),
        cover_color: SharedString::from(cover_hex),
        cover_color_value,
        has_cover_color,
        availability_schedule: SharedString::from(p.availability_schedule.as_deref().unwrap_or("")),
        discount_kind: SharedString::from(p.discount_kind.as_deref().unwrap_or("")),
        discount_value: SharedString::from(
            p.discount_value.map(|v| format!("{v}")).unwrap_or_default()
        ),
        discount_min_qty: SharedString::from(
            p.discount_min_qty.map(|v| format!("{v}")).unwrap_or_default()
        ),
        discount_tiers: SharedString::from(p.discount_tiers.as_deref().unwrap_or("")),
        addon_group_ids: SharedString::from(addon_group_ids_to_csv(&p.addon_group_ids)),
        variations: SharedString::from(p.variations.as_deref().unwrap_or("")),
        product_image: pixel_buf.map(slint::Image::from_rgba8).unwrap_or_default(),
    }
}

/// Insere um ProductData no início do modelo sem reload completo.
///
/// Regras aplicadas (AI_RULES.md §8, §13): atualização cirúrgica — sem re-decode.
///
/// Mantém a mesma ordem de `find_all` (`ORDER BY created_at DESC`):
/// produtos recém-criados/duplicados aparecem no topo da lista, não no
/// rodapé. Antes era `vm.push`, o que jogava o novo produto no fim e
/// criava inconsistência depois do próximo refresh.
pub(crate) fn push_product_to_model(ui: &MainWindow, data: ProductData) {
    let model = ui.get_products();
    if let Some(vm) = model.as_any().downcast_ref::<VecModel<ProductData>>() {
        vm.insert(0, data);
    }
}

/// Substitui um produto no modelo pelo ID sem reload completo.
///
/// Regras aplicadas (AI_RULES.md §8, §13): atualização cirúrgica — sem re-decode.
/// Usa remove + insert (não set_row_data) para garantir re-avaliação das condições
/// `if product.product-image.width` no ProductCard.
pub(crate) fn replace_product_in_model(ui: &MainWindow, id: &SharedString, data: ProductData) {
    let model = ui.get_products();
    if let Some(vm) = model.as_any().downcast_ref::<VecModel<ProductData>>() {
        for i in 0..vm.row_count() {
            if vm.row_data(i).map(|p| p.id == id).unwrap_or(false) {
                vm.remove(i);
                vm.insert(i, data);
                return;
            }
        }
    }
}

/// Atualiza in-place o `active` ou `web_visible` de um produto no
/// modelo. Faz `remove + insert` (não `set_row_data`) para que os
/// `if product.active: ...` e `if product.web-visible: ...` no
/// ProductCard sejam re-avaliados — `set_row_data` propaga update
/// das propriedades, mas não força re-avaliação dos `if` condicionais
/// que controlam a visibilidade dos badges do card.
pub(crate) fn update_product_flag(ui: &MainWindow, id: &SharedString, update: impl FnOnce(&mut ProductData)) {
    let model = ui.get_products();
    if let Some(vm) = model.as_any().downcast_ref::<VecModel<ProductData>>() {
        for i in 0..vm.row_count() {
            if let Some(mut p) = vm.row_data(i) {
                if p.id == id {
                    update(&mut p);
                    vm.remove(i);
                    vm.insert(i, p);
                    return;
                }
            }
        }
    }
}

/// Atualiza in-place o `detail-product` se for o produto exibido no
/// painel direito. Necessário para que o toggle `Ativo`/`Visível na
/// Web` reflita imediatamente no `LabeledSwitch` (que lê de
/// `detail-product.active`/`web-visible`) — sem isso, o switch só
/// mudava após sair e voltar na tela.
pub(crate) fn update_detail_product_flag(ui: &MainWindow, id: &SharedString, update: impl FnOnce(&mut ProductData)) {
    let mut current = ui.get_detail_product();
    if current.id == id {
        update(&mut current);
        ui.set_detail_product(current);
    }
}

