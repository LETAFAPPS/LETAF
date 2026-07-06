
use slint::{Color, Image, ModelRc, SharedString, VecModel};

use letaf_core::category::model::Category;
use letaf_core::product::model::Product;

use crate::format::format_stock;
use crate::{InventoryHealthData, InventoryProductRow, MainWindow};

use super::super::image::decode_pixel_buffer;
use super::setup::{SharedCache, SharedCategories};

// ── Pintura: derivações + populate UI ────────────────────────────

pub(crate) fn apply_to_ui_from_cache(
    ui_weak: &slint::Weak<MainWindow>,
    cache: &SharedCache,
    cats_cache: &SharedCategories,
) {
    let ui_weak = ui_weak.clone();
    let cache_snapshot = cache.lock().ok().map(|g| g.clone()).unwrap_or_default();
    let cats_snapshot = cats_cache.lock().ok().map(|g| g.clone()).unwrap_or_default();
    let _ = slint::invoke_from_event_loop(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        apply_to_ui(&ui, &cache_snapshot, &cats_snapshot);
    });
}

pub(crate) fn apply_to_ui(ui: &MainWindow, products: &[Product], categories: &[Category]) {
    // Métricas (sobre o conjunto completo — não filtrado).
    let total = products.len();
    let mut out_count = 0_i64;
    let mut low_count = 0_i64;
    let mut ok_count = 0_i64;
    let mut sync_pending = 0_i64;
    for p in products {
        if !p.base.synced {
            sync_pending += 1;
        }
        if p.unlimited_stock {
            ok_count += 1;
            continue;
        }
        match status_key(p) {
            "out" => out_count += 1,
            "low" => low_count += 1,
            "ok" => ok_count += 1,
            _ => {}
        }
    }
    let (health_status, health_label) = derive_health(total as i64, out_count, low_count, ok_count);
    let health = InventoryHealthData {
        health_status: SharedString::from(health_status),
        health_label: SharedString::from(health_label),
        // `health_counts` ficou no struct mas não é exibido (UI removeu).
        health_counts: SharedString::default(),
        out_count: out_count as i32,
        low_count: low_count as i32,
        sync_pending_count: sync_pending as i32,
        total_count: total as i32,
    };
    ui.set_inventory_health(health);

    // Filtro de busca (nome) + filtro de status (abas). As métricas
    // acima continuam refletindo o conjunto COMPLETO; só a lista
    // exibida é filtrada.
    let search_lc = ui.get_inventory_search().to_string().trim().to_lowercase();
    let filter = ui.get_inventory_filter().to_string();
    let mut filtered: Vec<&Product> = products
        .iter()
        .filter(|p| {
            let name_ok = search_lc.is_empty() || p.name.to_lowercase().contains(&search_lc);
            let status_ok = match filter.as_str() {
                "out" => status_key(p) == "out",
                "low" => status_key(p) == "low",
                "ok" => matches!(status_key(p), "ok" | "unlimited"),
                _ => true,
            };
            name_ok && status_ok
        })
        .collect();
    // Ordena por grupo (Sem Estoque → Baixo → Saudável) e, dentro do
    // grupo, por quantidade ASCENDENTE (menos estoque primeiro) e nome.
    filtered.sort_by(|a, b| {
        priority_of(status_key(a))
            .cmp(&priority_of(status_key(b)))
            .then_with(|| {
                a.stock_quantity
                    .partial_cmp(&b.stock_quantity)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    // Referência da barra de progresso + contagem por grupo.
    let max_qty = filtered
        .iter()
        .filter(|p| !p.unlimited_stock)
        .map(|p| p.stock_quantity)
        .fold(0.0_f64, f64::max);
    let (mut count_out, mut count_low, mut count_ok) = (0_i32, 0_i32, 0_i32);
    for p in &filtered {
        match group_of(status_key(p)) {
            "out" => count_out += 1,
            "low" => count_low += 1,
            _ => count_ok += 1,
        }
    }

    // Marca a 1ª linha de cada grupo com o cabeçalho (rótulo + contagem).
    let mut product_rows: Vec<InventoryProductRow> = Vec::with_capacity(filtered.len());
    let mut prev_group = "";
    for p in &filtered {
        let group = group_of(status_key(p));
        let mut row = product_to_row(p, categories, max_qty);
        if group != prev_group {
            let (label, color) = group_meta(group);
            row.group_first = true;
            row.group_label = SharedString::from(label);
            row.group_count = match group {
                "out" => count_out,
                "low" => count_low,
                _ => count_ok,
            };
            row.group_color = color;
            prev_group = group;
        }
        product_rows.push(row);
    }
    ui.set_inventory_products(ModelRc::new(VecModel::from(product_rows)));
}

/// Prioridade de exibição dentro da coluna. `ok` antes de `unlimited`
/// (mesma coluna "Saudável"); demais não importam porque cada coluna
/// filtra por status — só ok/unlimited convivem.
pub(crate) fn priority_of(status: &str) -> u8 {
    match status {
        "out" => 0,
        "low" => 1,
        "ok" => 2,
        "unlimited" => 3,
        _ => 50,
    }
}

pub(crate) fn product_to_row(p: &Product, categories: &[Category], max_qty: f64) -> InventoryProductRow {
    let cat_name = p
        .category_id
        .and_then(|cid| categories.iter().find(|c| c.base.id == cid))
        .map(|c| c.name.clone())
        .unwrap_or_else(|| "Sem categoria".into());
    let cat_unit = format!("{} · {}", cat_name, p.unit);
    let qty_display = if p.unlimited_stock {
        "Ilimitado".to_string()
    } else {
        format!("{} {}", format_stock(p.stock_quantity, &p.unit), p.unit)
    };
    let min_display = if p.unlimited_stock {
        String::new()
    } else if p.min_stock > 0.0 {
        format!("mínimo {}", format_stock(p.min_stock, &p.unit))
    } else {
        "sem mínimo".into()
    };
    let status = status_key(p).to_string();
    let status_label = match status.as_str() {
        "out" => "Sem Estoque",
        "low" => "Estoque Baixo",
        "ok" => "Saudável",
        _ => "Ilimitado",
    };
    let initials = make_initials(&p.name);
    let avatar_color = color_for_name(&cat_name);
    let (product_image, has_image) = decode_product_thumb(p.image_data.as_deref());
    InventoryProductRow {
        id: SharedString::from(p.base.id.to_string()),
        name: SharedString::from(p.name.clone()),
        product_image,
        has_image,
        initials: SharedString::from(initials),
        avatar_color,
        category_label: SharedString::from(cat_unit),
        qty_display: SharedString::from(qty_display),
        min_display: SharedString::from(min_display),
        status: SharedString::from(status),
        status_label: SharedString::from(status_label),
        sync_label: SharedString::from(if p.base.synced { "" } else { "Aguardando sync" }),
        fill_ratio: fill_ratio(p, max_qty),
        group_first: false,
        group_label: SharedString::default(),
        group_count: 0,
        group_color: Color::default(),
    }
}

/// Grupo de exibição da lista: out / low / (ok+unlimited juntos).
pub(crate) fn group_of(status: &str) -> &'static str {
    match status {
        "out" => "out",
        "low" => "low",
        _ => "ok",
    }
}

/// Rótulo (caixa-alta) e cor do cabeçalho de cada grupo.
fn group_meta(group: &str) -> (&'static str, Color) {
    match group {
        "out" => ("SEM ESTOQUE", Color::from_rgb_u8(0xC6, 0x28, 0x28)),
        "low" => ("ESTOQUE BAIXO", Color::from_rgb_u8(0xF5, 0x7F, 0x17)),
        _ => ("SAUDÁVEL", Color::from_rgb_u8(0x2E, 0x7D, 0x32)),
    }
}

/// Proporção (0..1) da barra de progresso. Referência: 2× o mínimo
/// configurado (no mínimo = 50%); sem mínimo, 2× o maior estoque da
/// lista (escala suave). Zerado = 0; ilimitado = cheio.
fn fill_ratio(p: &Product, max_qty: f64) -> f32 {
    if p.unlimited_stock {
        return 1.0;
    }
    if p.stock_quantity <= 0.0 {
        return 0.0;
    }
    let reference = if p.min_stock > 0.0 {
        p.min_stock * 2.0
    } else if max_qty > 0.0 {
        max_qty * 2.0
    } else {
        p.stock_quantity
    };
    (p.stock_quantity / reference).clamp(0.05, 1.0) as f32
}

pub(crate) fn status_key(p: &Product) -> &'static str {
    if p.unlimited_stock {
        "unlimited"
    } else if p.stock_quantity <= 0.0 {
        "out"
    } else if p.min_stock > 0.0 && p.stock_quantity <= p.min_stock {
        "low"
    } else {
        "ok"
    }
}

pub(crate) fn derive_health(total: i64, out: i64, low: i64, _ok: i64) -> (&'static str, &'static str) {
    if total == 0 {
        return ("ok", "Saudável");
    }
    if out > 0 || (low as f64 / total as f64) > 0.30 {
        return ("critical", "Crítica");
    }
    if low > 0 {
        return ("attention", "Atenção");
    }
    ("ok", "Saudável")
}

pub(crate) fn make_initials(name: &str) -> String {
    let mut letters: Vec<char> = Vec::new();
    for part in name.split_whitespace() {
        if let Some(c) = part.chars().next() {
            letters.push(c.to_ascii_uppercase());
            if letters.len() == 2 {
                break;
            }
        }
    }
    if letters.is_empty() {
        return "?".into();
    }
    letters.into_iter().collect()
}

pub(crate) fn color_for_name(name: &str) -> Color {
    let palette = [
        (0xE5, 0x39, 0x35),
        (0xF9, 0xA8, 0x25),
        (0x43, 0xA0, 0x47),
        (0x1E, 0x88, 0xE5),
        (0x8E, 0x24, 0xAA),
        (0xFB, 0x8C, 0x00),
        (0x00, 0x89, 0x7B),
        (0xC2, 0x18, 0x5B),
    ];
    let mut h: u32 = 0;
    for b in name.as_bytes() {
        h = h.wrapping_mul(31).wrapping_add(*b as u32);
    }
    let (r, g, b) = palette[(h as usize) % palette.len()];
    Color::from_rgb_u8(r, g, b)
}

/// Decodifica a miniatura do produto (base64 → Image). Retorna
/// `(Image::default(), false)` quando o produto não tem imagem ou
/// falha ao decodificar — UI cai no avatar de iniciais.
pub(crate) fn decode_product_thumb(image_data: Option<&str>) -> (Image, bool) {
    let Some(b64) = image_data.filter(|s| !s.is_empty()) else {
        return (Image::default(), false);
    };
    match decode_pixel_buffer(b64) {
        Some(buf) => (Image::from_rgba8(buf), true),
        None => (Image::default(), false),
    }
}
