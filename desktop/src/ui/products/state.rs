use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use slint::{Model, ModelRc, SharedString};


use crate::BusinessHoursData;



/// Entrada serializada da agenda de disponibilidade (1 por dia).
///
/// Regras aplicadas (AI_RULES.md §1, §8):
/// - Vive na camada desktop porque o core não interpreta o JSON (mantém
///   só `Option<String>`). Web tem deserialização própria.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub(crate) struct AvailabilityDay {
    pub day: u8,
    pub open: String,
    pub close: String,
    pub active: bool,
}

/// Nomes dos dias em pt-BR (índice 0 = domingo).
const DAY_NAMES_PT_BR: [&str; 7] = [
    "Domingo", "Segunda", "Terça", "Quarta", "Quinta", "Sexta", "Sábado",
];

/// Default: 7 dias, todos ativos, 08:00–22:00.
pub(crate) fn default_availability_entries() -> Vec<AvailabilityDay> {
    (0..7u8).map(|d| AvailabilityDay {
        day: d,
        open: "08:00".to_string(),
        close: "22:00".to_string(),
        active: true,
    }).collect()
}

/// Parseia o JSON do banco. Em qualquer falha (corrompido, ausente)
/// retorna o default — UI fica utilizável.
pub(crate) fn parse_availability(json: Option<&str>) -> Vec<AvailabilityDay> {
    json.and_then(|s| serde_json::from_str::<Vec<AvailabilityDay>>(s).ok())
        .filter(|v| v.len() == 7)
        .unwrap_or_else(default_availability_entries)
}

/// Converte `[AvailabilityDay]` em `[BusinessHoursData]` para a UI Slint.
pub(crate) fn availability_to_ui(entries: &[AvailabilityDay]) -> Vec<BusinessHoursData> {
    entries.iter().map(|e| BusinessHoursData {
        id: SharedString::default(),
        day_of_week: e.day as i32,
        day_name: SharedString::from(
            DAY_NAMES_PT_BR.get(e.day as usize).copied().unwrap_or("")
        ),
        open_time: SharedString::from(e.open.as_str()),
        close_time: SharedString::from(e.close.as_str()),
        is_open: e.active,
    }).collect()
}

/// Lê a lista do Slint e serializa em JSON. Retorna `None` se `enabled=false`
/// (sempre disponível). Garante 7 entradas e ordem por `day_of_week`.
pub(crate) fn ui_to_availability_json(
    enabled: bool,
    model: &ModelRc<BusinessHoursData>,
) -> Option<String> {
    if !enabled { return None; }
    let mut entries: Vec<AvailabilityDay> = (0..model.row_count())
        .filter_map(|i| model.row_data(i))
        .map(|d| AvailabilityDay {
            day: d.day_of_week.clamp(0, 6) as u8,
            open: d.open_time.to_string(),
            close: d.close_time.to_string(),
            active: d.is_open,
        })
        .collect();
    if entries.len() != 7 {
        // Estado degenerado — cai para default para não persistir lixo.
        entries = default_availability_entries();
    }
    entries.sort_by_key(|e| e.day);
    serde_json::to_string(&entries).ok()
}


/// Estado dos filtros da grade de produtos.
///
/// Regras aplicadas (AI_RULES.md §1, §8, §11):
/// - Estado fica no Rust; a UI apenas reflete via `filter-cats` / `filter-subs`
///   e dispara callbacks. Set/HashMap evitam lógica de coleção no Slint.
/// - `selected_*` vazio significa "sem restrição" (mostra todas as categorias);
///   caso contrário, filtra apenas pelas IDs marcadas. Esse comportamento
///   bate com a UX de chips: nada selecionado = todos.
/// - Padrão `status = "active"` e `stock = "with"` reflete a regra do PDV:
///   ao abrir a tela, o operador vê apenas o que é vendável.
pub(crate) struct ProductFilterState {
    pub search_query: String,
    pub selected_categories: HashSet<String>,
    pub selected_subcategories: HashSet<String>,
    pub status: String,
    pub stock: String,
    /// Catálogo conhecido (id, name) — populado a cada refresh.
    pub known_categories: Vec<(String, String)>,
    /// Catálogo de subcategorias (id, category_id, name).
    pub known_subcategories: Vec<(String, String, String)>,
}

impl Default for ProductFilterState {
    fn default() -> Self {
        Self {
            search_query: String::new(),
            selected_categories: HashSet::new(),
            selected_subcategories: HashSet::new(),
            status: "both".to_string(),
            stock: "both".to_string(),
            known_categories: Vec::new(),
            known_subcategories: Vec::new(),
        }
    }
}

pub(crate) type SharedFilter = Arc<Mutex<ProductFilterState>>;

/// Produto com pixels já decodificados — thread-safe (Send).
///
/// Regras aplicadas (AI_RULES.md §8):
/// - slint::Image não implementa Send (usa VRc interno com *mut ())
/// - SharedPixelBuffer<Rgba8Pixel> implementa Send (Arc<[T]>)
/// - Essa struct permite que o decode CPU-bound rode em spawn_blocking
pub(crate) struct DecodedProduct {
    pub(crate) id: SharedString,
    pub(crate) name: SharedString,
    pub(crate) description: SharedString,
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
    pub(crate) unlimited_stock: bool,
    pub(crate) barcode: SharedString,
    pub(crate) unit: SharedString,
    pub(crate) active: bool,
    pub(crate) web_visible: bool,
    pub(crate) synced: bool,
    pub(crate) sync_label: SharedString,
    pub(crate) balance_mode: SharedString,
    pub(crate) image_data: SharedString,
    pub(crate) category_id: SharedString,
    pub(crate) category_name: SharedString,
    pub(crate) subcategory_id: SharedString,
    pub(crate) subcategory_name: SharedString,
    pub(crate) cover_color: SharedString,
    /// Pré-parsado em RGB para o Slint (ele não tem `color.from-string`).
    pub(crate) cover_color_rgb: Option<(u8, u8, u8)>,
    /// JSON cru da agenda de disponibilidade (vazio = sempre disponível).
    pub(crate) availability_schedule: SharedString,
    pub(crate) discount_kind: SharedString,
    pub(crate) discount_value: SharedString,
    pub(crate) discount_min_qty: SharedString,
    pub(crate) discount_tiers: SharedString,
    pub(crate) addon_group_ids: SharedString,
    pub(crate) variations: SharedString,
    pub(crate) pixel_buffer: Option<slint::SharedPixelBuffer<slint::Rgba8Pixel>>,
}

