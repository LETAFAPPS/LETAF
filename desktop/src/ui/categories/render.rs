use std::collections::{HashMap, HashSet};
use rust_decimal::prelude::ToPrimitive;

use slint::{Color, ModelRc, SharedString, VecModel};
use uuid::Uuid;

use letaf_core::category::model::Category;
use letaf_core::product::model::Product;
use letaf_core::subcategory::model::Subcategory;

use crate::{CatProduct, CatSub, CatTreeRow, CategoryData, MainWindow};

use super::super::image::decode_pixel_buffer;
use super::crud::to_category_data;

/// Cache do master-detail de Categorias. Mantém o snapshot do SQLite e
/// o estado de UI (expansão da árvore e seleção) para que o detalhe
/// possa ser reagregado sem novas queries e a seleção sobreviva a um
/// refresh (criação/edição/exclusão).
#[derive(Default)]
pub(crate) struct CatCache {
    pub(crate) categories: Vec<Category>,
    pub(crate) subcategories: Vec<Subcategory>,
    pub(crate) products: Vec<Product>,
    pub(crate) expanded: HashSet<Uuid>,
    pub(crate) selected: Option<Uuid>,
}

/// Paleta determinística (índice da categoria → cor). Sem cor no
/// domínio: pintamos de forma estável a partir da posição (§1/§3/§8).
pub(crate) fn cat_color(idx: usize) -> Color {
    const PALETTE: &[(u8, u8, u8)] = &[
        (0xB0, 0x7D, 0x42), // marrom
        (0xE5, 0x5C, 0x3C), // vermelho
        (0xE0, 0xA8, 0x2E), // dourado
        (0x3B, 0x82, 0xF6), // azul
        (0xCC, 0x9A, 0x06), // amarelo escuro
        (0x8B, 0x5C, 0xF6), // roxo
        (0x16, 0xA3, 0x4A), // verde
        (0xEC, 0x48, 0x99), // rosa
        (0x0E, 0xA5, 0xE9), // ciano
        (0xF5, 0x7C, 0x00), // laranja
    ];
    let (r, g, b) = PALETTE[idx % PALETTE.len()];
    Color::from_rgb_u8(r, g, b)
}

/// Cor de capa do produto (`#RRGGBB`) → (Color, true) ou neutra.
pub(crate) fn product_color(hex: Option<&str>) -> (Color, bool) {
    if let Some(h) = hex {
        let h = h.trim_start_matches('#');
        if h.len() == 6 {
            if let (Ok(r), Ok(g), Ok(b)) = (
                u8::from_str_radix(&h[0..2], 16),
                u8::from_str_radix(&h[2..4], 16),
                u8::from_str_radix(&h[4..6], 16),
            ) {
                return (Color::from_rgb_u8(r, g, b), true);
            }
        }
    }
    (Color::from_rgb_u8(0, 0, 0), false)
}

/// Formata moeda em pt-BR: "R$ 2.530,00". Delega ao helper canônico
/// (era cópia byte-a-byte de `crate::format::money_br` — AI_RULES §8).
pub(crate) fn money_br(v: f64) -> String {
    crate::format::money_br_f64(v)
}

/// Índice da categoria na ordem de exibição (para a cor estável).
pub(crate) fn category_index(cache: &CatCache, id: Uuid) -> usize {
    cache
        .categories
        .iter()
        .position(|c| c.base.id == id)
        .unwrap_or(0)
}

/// Constrói as linhas da lista mestra (apenas categorias — sem
/// subcategorias/flecha/cor, pedido do usuário).
pub(crate) fn build_tree(cache: &CatCache) -> Vec<CatTreeRow> {
    cache
        .categories
        .iter()
        .enumerate()
        .map(|(idx, c)| CatTreeRow {
            id: SharedString::from(c.base.id.to_string()),
            name: SharedString::from(c.name.as_str()),
            kind: SharedString::from("category"),
            color: cat_color(idx),
            expanded: false,
            has_children: false,
            has_icon: c.icon_name.as_deref().is_some_and(|s| !s.is_empty()),
        })
        .collect()
}

/// Resolve um id de linha (categoria OU subcategoria) para a
/// categoria correspondente.
pub(crate) fn resolve_category(cache: &CatCache, id: Uuid) -> Option<Uuid> {
    if cache.categories.iter().any(|c| c.base.id == id) {
        return Some(id);
    }
    cache
        .subcategories
        .iter()
        .find(|s| s.base.id == id)
        .map(|s| s.category_id)
}

/// Popula os campos de detalhe da categoria selecionada.
pub(crate) fn apply_detail(ui: &MainWindow, cache: &CatCache) {
    let Some(sel) = cache.selected.and_then(|id| resolve_category(cache, id)) else {
        ui.set_selected_category_id(SharedString::default());
        return;
    };
    let Some(cat) = cache.categories.iter().find(|c| c.base.id == sel) else {
        ui.set_selected_category_id(SharedString::default());
        return;
    };

    // Subcategorias da categoria.
    let mut subs: Vec<&Subcategory> = cache
        .subcategories
        .iter()
        .filter(|s| s.category_id == sel)
        .collect();
    subs.sort_by_key(|s| s.sort_order);
    let sub_name: HashMap<Uuid, String> =
        subs.iter().map(|s| (s.base.id, s.name.clone())).collect();

    // Produtos ativos da categoria.
    let mut prods: Vec<&Product> = cache
        .products
        .iter()
        .filter(|p| p.active && p.category_id == Some(sel))
        .collect();
    prods.sort_by_key(|a| a.name.to_lowercase());

    let total_active = cache
        .products
        .iter()
        .filter(|p| p.active && p.category_id.is_some())
        .count();

    let stock_value: f64 = prods
        .iter()
        .filter(|p| !p.unlimited_stock)
        .map(|p| p.price.map(|d| d.to_f64().unwrap_or(0.0)).unwrap_or(0.0) * p.stock_quantity)
        .sum();

    let prod_rows: Vec<CatProduct> = prods
        .iter()
        .map(|p| {
            let (color, has_color) = product_color(p.cover_color.as_deref());
            let qty = if p.unlimited_stock {
                "Ilimitado".to_string()
            } else {
                format!("{} {}", p.stock_quantity as i64, p.unit)
            };
            let sub = p
                .subcategory_id
                .and_then(|sid| sub_name.get(&sid).cloned())
                .unwrap_or_else(|| "".to_string());
            // Miniatura: decodifica a imagem do produto. apply_detail
            // roda na thread da UI (slint::Image só pode ser criado
            // aqui); listas por categoria são pequenas.
            let pix = p
                .image_data
                .as_deref()
                .filter(|s| !s.is_empty())
                .and_then(decode_pixel_buffer);
            let has_image = pix.is_some();
            let product_image = pix
                .map(slint::Image::from_rgba8)
                .unwrap_or_default();
            CatProduct {
                id: SharedString::from(p.base.id.to_string()),
                name: SharedString::from(p.name.as_str()),
                product_image,
                has_image,
                sub_name: SharedString::from(sub),
                qty: SharedString::from(qty),
                price: SharedString::from(money_br(p.price.map(|d| d.to_f64().unwrap_or(0.0)).unwrap_or(0.0))),
                color,
                has_color,
            }
        })
        .collect();

    let sub_rows: Vec<CatSub> = subs
        .iter()
        .map(|s| {
            let n = cache
                .products
                .iter()
                .filter(|p| p.active && p.subcategory_id == Some(s.base.id))
                .count();
            CatSub {
                id: SharedString::from(s.base.id.to_string()),
                name: SharedString::from(s.name.as_str()),
                category_id: SharedString::from(s.category_id.to_string()),
                category_name: SharedString::from(cat.name.as_str()),
                count_label: SharedString::from(format!(
                    "{n} Produto{}",
                    if n == 1 { "" } else { "s" }
                )),
            }
        })
        .collect();

    let pct = if total_active > 0 {
        (prod_rows.len() as f64 / total_active as f64 * 100.0).round() as i64
    } else {
        0
    };

    ui.set_selected_category_id(SharedString::from(sel.to_string()));
    ui.set_detail_category(to_category_data(cat));
    ui.set_detail_color(cat_color(category_index(cache, sel)));
    ui.set_detail_prod_count(prod_rows.len() as i32);
    ui.set_detail_sub_count(sub_rows.len() as i32);
    ui.set_detail_stock_value(SharedString::from(money_br(stock_value)));
    ui.set_detail_catalog_pct(SharedString::from(format!("{pct}%")));
    ui.set_detail_catalog_hint(SharedString::from(format!(
        "{} de {} Produtos",
        prod_rows.len(),
        total_active
    )));
    ui.set_detail_cat_products(ModelRc::new(VecModel::from(prod_rows)));
    ui.set_detail_cat_subs(ModelRc::new(VecModel::from(sub_rows)));
}

/// Reflete o cache na UI (árvore + detalhe + lista auxiliar usada
/// pelos pickers do formulário de produto/subcategoria).
pub(crate) fn apply_cache(ui: &MainWindow, cache: &CatCache) {
    let tree = build_tree(cache);
    ui.set_category_tree(ModelRc::new(VecModel::from(tree)));
    ui.set_category_count(cache.categories.len() as i32);
    let cats: Vec<CategoryData> = cache.categories.iter().map(to_category_data).collect();
    ui.set_categories(ModelRc::new(VecModel::from(cats)));
    apply_detail(ui, cache);
}

