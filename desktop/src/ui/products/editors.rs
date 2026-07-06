
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};


use crate::context::DesktopState;
use crate::{DiscountTierData, MainWindow, VariationData, VariationOptionData};

use super::state::{availability_to_ui, default_availability_entries, parse_availability};
use super::form::{parse_decimal, parse_variations_for_ui};
use super::data::parse_addon_group_ids_csv;

/// Popula `product-addon-groups` (chips) ao abrir o form. Recebe CSV
/// de UUIDs dos grupos JÁ associados; entradas com `selected = true`
/// representam essa associação. CSV vazio = nenhum marcado.
pub(crate) fn setup_load_product_addon_groups(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
) {
    let state = state.clone();
    let handle = handle.clone();
    let ui_weak = ui.as_weak();
    ui.on_load_product_addon_groups(move |csv| {
        let Some(ui) = ui_weak.upgrade() else { return };
        let selected = parse_addon_group_ids_csv(csv.as_str());
        super::super::addons::refresh_product_addon_groups(&ui, &state, &handle, selected);
    });
}

/// Popula `product-availability` ao abrir o formulário de edição.
///
/// Regras aplicadas (AI_RULES.md §1, §8):
/// - O Slint chama este callback passando o JSON cru (vazio quando o
///   produto é "sempre disponível"). Aqui parseamos e expomos a lista
///   de 7 `BusinessHoursData` para o card mostrar.
pub(crate) fn setup_load_product_availability(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_load_product_availability(move |json| {
        let Some(ui) = ui_weak.upgrade() else { return };
        let entries = parse_availability(
            if json.is_empty() { None } else { Some(json.as_str()) }
        );
        ui.set_product_availability(ModelRc::new(VecModel::from(availability_to_ui(&entries))));
    });
}

/// Popula `variations` ao abrir o form de edição.
pub(crate) fn setup_load_product_variations(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_load_product_variations(move |json| {
        let Some(ui) = ui_weak.upgrade() else { return };
        let list = parse_variations_for_ui(json.as_str());
        ui.set_product_variations(ModelRc::new(VecModel::from(list)));
    });
}

/// Insere uma variação em branco no fim da lista. Default é `single`
/// para alinhar com a UI mostrando a pill "Única" ativa.
pub(crate) fn setup_add_variation(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_add_variation(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        let model = ui.get_product_variations();
        let new_variation = VariationData {
            title: SharedString::default(),
            selection: SharedString::from("single"),
            required: false,
            min_select: SharedString::default(),
            max_select: SharedString::default(),
            options: ModelRc::new(VecModel::<VariationOptionData>::from(Vec::new())),
        };
        if let Some(vm) = model.as_any().downcast_ref::<VecModel<VariationData>>() {
            vm.push(new_variation);
        } else {
            let mut acc: Vec<VariationData> = (0..model.row_count())
                .filter_map(|i| model.row_data(i)).collect();
            acc.push(new_variation);
            ui.set_product_variations(ModelRc::new(VecModel::from(acc)));
        }
    });
}

pub(crate) fn setup_remove_variation(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_remove_variation(move |idx| {
        let Some(ui) = ui_weak.upgrade() else { return };
        let model = ui.get_product_variations();
        if idx < 0 || (idx as usize) >= model.row_count() { return; }
        if let Some(vm) = model.as_any().downcast_ref::<VecModel<VariationData>>() {
            vm.remove(idx as usize);
        }
    });
}

/// Adiciona opção em branco na variação `v_idx`. Como `options` é
/// `ModelRc<VariationOptionData>` (sub-modelo), mutamos in-place via
/// downcast e depois fazemos `set_row_data` na variação para que o
/// Slint re-avalie o `for option[o_idx] in variation.options`.
pub(crate) fn setup_add_variation_option(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_add_variation_option(move |v_idx| {
        let Some(ui) = ui_weak.upgrade() else { return };
        let model = ui.get_product_variations();
        if v_idx < 0 || (v_idx as usize) >= model.row_count() { return; }
        let Some(variation) = model.row_data(v_idx as usize) else { return };
        let new_opt = VariationOptionData {
            name: SharedString::default(),
            price: SharedString::default(),
        };
        if let Some(opts_vm) = variation.options.as_any().downcast_ref::<VecModel<VariationOptionData>>() {
            opts_vm.push(new_opt);
        } else {
            // Reconstrói o sub-VecModel se veio como literal Slint.
            let mut acc: Vec<VariationOptionData> = (0..variation.options.row_count())
                .filter_map(|i| variation.options.row_data(i)).collect();
            acc.push(new_opt);
            let updated = VariationData {
                title: variation.title.clone(),
                selection: variation.selection.clone(),
                required: variation.required,
                min_select: variation.min_select.clone(),
                max_select: variation.max_select.clone(),
                options: ModelRc::new(VecModel::from(acc)),
            };
            model.set_row_data(v_idx as usize, updated);
        }
    });
}

pub(crate) fn setup_remove_variation_option(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_remove_variation_option(move |v_idx, o_idx| {
        let Some(ui) = ui_weak.upgrade() else { return };
        let model = ui.get_product_variations();
        if v_idx < 0 || (v_idx as usize) >= model.row_count() { return; }
        let Some(variation) = model.row_data(v_idx as usize) else { return };
        if o_idx < 0 || (o_idx as usize) >= variation.options.row_count() { return; }
        if let Some(opts_vm) = variation.options.as_any().downcast_ref::<VecModel<VariationOptionData>>() {
            opts_vm.remove(o_idx as usize);
        }
    });
}

/// Inicializa `product-availability` com 7 entradas default ao subir a
/// aplicação — garante que o card de Disponibilidade tem dados para
/// mostrar mesmo em "Novo Produto".
pub(crate) fn init_product_availability_default(ui: &MainWindow) {
    let entries = default_availability_entries();
    ui.set_product_availability(ModelRc::new(VecModel::from(availability_to_ui(&entries))));
}

/// Adiciona uma faixa de desconto bulk em branco ao final da lista.
/// Reaproveita o `VecModel` existente para preservar foco/scroll.
pub(crate) fn setup_add_discount_tier(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_add_discount_tier(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        let model = ui.get_product_discount_tiers();
        if let Some(vm) = model.as_any().downcast_ref::<VecModel<DiscountTierData>>() {
            vm.push(DiscountTierData {
                min_qty: SharedString::default(),
                value: SharedString::default(),
            });
        } else {
            // Lista veio de fora do VecModel (literal Slint []). Constrói
            // um novo VecModel com o que existe + nova entrada e troca.
            let mut acc: Vec<DiscountTierData> = (0..model.row_count())
                .filter_map(|i| model.row_data(i))
                .collect();
            acc.push(DiscountTierData {
                min_qty: SharedString::default(),
                value: SharedString::default(),
            });
            ui.set_product_discount_tiers(ModelRc::new(VecModel::from(acc)));
        }
    });
}

/// Remove a faixa de desconto no índice indicado. Validação de "ao
/// menos um tier para bulk" fica no service no save.
pub(crate) fn setup_remove_discount_tier(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_remove_discount_tier(move |idx| {
        let Some(ui) = ui_weak.upgrade() else { return };
        let model = ui.get_product_discount_tiers();
        if idx < 0 || (idx as usize) >= model.row_count() { return; }
        if let Some(vm) = model.as_any().downcast_ref::<VecModel<DiscountTierData>>() {
            vm.remove(idx as usize);
        }
    });
}

/// Popula `product-discount-tiers` com base no JSON persistido no
/// produto. JSON inválido ou vazio resulta em lista vazia.
pub(crate) fn setup_load_discount_tiers(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_load_discount_tiers(move |json| {
        let Some(ui) = ui_weak.upgrade() else { return };
        let tiers = parse_tiers_for_ui(json.as_str());
        ui.set_product_discount_tiers(ModelRc::new(VecModel::from(tiers)));
    });
}

/// Parseia o JSON `[{"min_qty", "value"}, ...]` em `DiscountTierData`
/// (strings — a UI mostra texto bruto). Falhas retornam lista vazia.
fn parse_tiers_for_ui(json: &str) -> Vec<DiscountTierData> {
    if json.trim().is_empty() { return Vec::new(); }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let Some(arr) = v.as_array() else { return Vec::new(); };
    arr.iter()
        .filter_map(|item| {
            let obj = item.as_object()?;
            let q = obj.get("min_qty")?.as_f64()?;
            let val = obj.get("value")?.as_f64()?;
            Some(DiscountTierData {
                min_qty: SharedString::from(format_tier_number(q)),
                value: SharedString::from(format_tier_number(val)),
            })
        })
        .collect()
}

/// `1.0` → `"1"`, `2.5` → `"2.5"` — evita ruído de `1` virar `1.0` na UI.
fn format_tier_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1.0e15 {
        format!("{:.0}", n)
    } else {
        format!("{n}")
    }
}

/// Lê `product-discount-tiers` da UI e serializa em JSON pronto para o
/// service. Tiers com qualquer campo vazio/inválido são descartados;
/// o resultado é ordenado por `min_qty` crescente. Retorna `None` se
/// nenhum tier válido sobrou (o caller decide o que fazer — service
/// rejeita `bulk_*` sem tiers).
pub(crate) fn ui_tiers_to_json(ui: &MainWindow) -> Option<String> {
    let model = ui.get_product_discount_tiers();
    let mut pairs: Vec<(f64, f64)> = (0..model.row_count())
        .filter_map(|i| model.row_data(i))
        .filter_map(|t| {
            let q = parse_decimal(&t.min_qty)?;
            let v = parse_decimal(&t.value)?;
            Some((q, v))
        })
        .collect();
    if pairs.is_empty() { return None; }
    pairs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let arr: Vec<serde_json::Value> = pairs
        .into_iter()
        .map(|(q, v)| serde_json::json!({ "min_qty": q, "value": v }))
        .collect();
    serde_json::to_string(&serde_json::Value::Array(arr)).ok()
}

