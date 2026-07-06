
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use rust_decimal::prelude::ToPrimitive;
use uuid::Uuid;


use crate::context::DesktopState;

use crate::MainWindow;

use super::super::helpers::show_toast;

// ── Configurador de produto (adicionais/variações) ─────────────────
//
// Quando o operador escolhe um produto no picker, decidimos se
// abrimos o configurador ou adicionamos direto. Para escolha
// "single", o toggle desmarca os irmãos no mesmo grupo/variação.
// Para "max_value", o preço unitário só inclui o MAIOR valor entre
// os marcados (semântica do backend — vide
// `web/components/addon_selector::build_variations_snapshot`).

#[derive(serde::Deserialize, Debug, Clone)]
struct VariationOptionJson { name: String, price: f64 }
#[derive(serde::Deserialize, Debug, Clone)]
struct VariationJson {
    title: String,
    #[serde(default)] selection: String,
    #[serde(default)] required: bool,
    #[serde(default)] min_select: i64,
    #[serde(default)] max_select: i64,
    #[serde(default)] options: Vec<VariationOptionJson>,
}

fn format_addon_price(p: f64) -> String {
    if p <= 0.0 { "Grátis".into() } else { format!("+ R$ {:.2}", p) }
}

fn variation_hint(v: &VariationJson) -> String {
    let mut parts = Vec::new();
    if v.required { parts.push("Obrigatório".to_string()); }
    if v.min_select > 0 && v.max_select > 0 && v.min_select == v.max_select {
        parts.push(format!("Escolha {}", v.min_select));
    } else {
        if v.min_select > 0 { parts.push(format!("min. {}", v.min_select)); }
        if v.max_select > 0 { parts.push(format!("máx. {}", v.max_select)); }
    }
    if v.selection == "max_value" {
        parts.push("cobra o maior".to_string());
    }
    parts.join(" · ")
}

fn addon_group_hint(g: &letaf_core::addon_group::model::AddonGroup) -> String {
    let mut parts = Vec::new();
    if g.min_select > 0 && g.max_select > 0 && g.min_select == g.max_select {
        parts.push(format!("Escolha {}", g.min_select));
    } else {
        if g.min_select > 0 { parts.push(format!("min. {}", g.min_select)); }
        if g.max_select > 0 { parts.push(format!("máx. {}", g.max_select)); }
    }
    parts.join(" · ")
}

/// Recomputa `config-final-display` e atualiza `unit-price` dos rows
/// quando o operador altera seleção ou qty.
fn recompute_config_total(ui: &MainWindow) {
    let base = ui.get_config_base_price() as f64;
    let qty = ui.get_config_qty().max(1) as f64;
    // Variações: sum dos marcados (com regra max_value).
    let mut extras = 0.0_f64;
    let variations = ui.get_config_variations();
    for vi in 0..variations.row_count() {
        let v = variations.row_data(vi).unwrap();
        let selection = v.selection.to_string();
        let opts = v.options;
        if selection == "max_value" {
            let mut max_p = 0.0_f64;
            for oi in 0..opts.row_count() {
                let o = opts.row_data(oi).unwrap();
                if o.selected && (o.price as f64) > max_p { max_p = o.price as f64; }
            }
            extras += max_p;
        } else {
            for oi in 0..opts.row_count() {
                let o = opts.row_data(oi).unwrap();
                if o.selected { extras += o.price as f64; }
            }
        }
    }
    // Adicionais: soma direta dos marcados (sem regra max_value).
    let groups = ui.get_config_addon_groups();
    for gi in 0..groups.row_count() {
        let g = groups.row_data(gi).unwrap();
        let addons = g.addons;
        for ai in 0..addons.row_count() {
            let a = addons.row_data(ai).unwrap();
            if a.selected { extras += a.price as f64; }
        }
    }
    let total = (base + extras) * qty;
    ui.set_config_final_display(SharedString::from(format!("R$ {:.2}", total)));
}

/// "Comecar a configurar" — chama Rust quando o operador clica num
/// produto no picker. Decide: se o produto tem adicionais OU
/// variações, abre o configurador; senão, adiciona direto.
pub(crate) fn setup_start_product_config(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_start_product_config(move |pid| {
        let pid_uuid = match Uuid::parse_str(pid.as_str()) {
            Ok(v) => v, Err(_) => return,
        };
        let state2 = state.clone();
        let ui_weak2 = ui_weak.clone();
        let pid_str = pid.to_string();
        handle.spawn(async move {
            let cid = state2.company_id();
            let product = match state2.product_service.find_by_id(cid, pid_uuid).await {
                Ok(Some(p)) => p,
                _ => return,
            };
            let has_variations = product.variations.as_deref()
                .map(|s| !s.trim().is_empty() && s.trim() != "[]")
                .unwrap_or(false);
            let has_addons = !product.addon_group_ids.is_empty();
            // Sem complexidade: empurra direto no model (rota antiga).
            if !has_variations && !has_addons {
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = ui_weak2.upgrade() else { return };
                    ui.invoke_edit_order_add_product(SharedString::from(pid_str));
                });
                return;
            }
            // Carrega addon groups + addons em paralelo.
            let mut groups: Vec<(letaf_core::addon_group::model::AddonGroup, Vec<letaf_core::addon::model::Addon>)> = Vec::new();
            for gid in &product.addon_group_ids {
                if let Ok(Some(g)) = state2.addon_group_service.find_by_id(cid, *gid).await {
                    let addons = state2.addon_service.find_by_group(cid, *gid).await.unwrap_or_default();
                    groups.push((g, addons));
                }
            }
            // Parseia variações.
            let variations_parsed: Vec<VariationJson> = product.variations
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default();
            // Desconto eventual já aplicado (fixed/percent/bulk com qty=1).
            // Mesma lógica do server em `verify_item_prices` — evita
            // "Price mismatch" no sync.
            let base_price = letaf_core::discount::effective_unit_price(&product, 1.0);
            let product_name = product.name.clone();
            // Move para o event loop e monta os modelos Slint.
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak2.upgrade() else { return };
                let vars_ui: Vec<crate::ConfigVariation> = variations_parsed.into_iter().map(|v| {
                    let hint = variation_hint(&v);
                    let opts: Vec<crate::ConfigOption> = v.options.iter().map(|o| crate::ConfigOption {
                        name: SharedString::from(o.name.as_str()),
                        price: o.price as f32,
                        price_display: SharedString::from(format_addon_price(o.price)),
                        selected: false,
                    }).collect();
                    crate::ConfigVariation {
                        title: SharedString::from(v.title.clone()),
                        selection: SharedString::from(v.selection.clone()),
                        hint: SharedString::from(hint),
                        required: v.required,
                        min_select: v.min_select as i32,
                        max_select: v.max_select as i32,
                        options: ModelRc::new(VecModel::from(opts)),
                    }
                }).collect();
                let groups_ui: Vec<crate::ConfigAddonGroup> = groups.iter().map(|(g, addons)| {
                    let hint = addon_group_hint(g);
                    let items: Vec<crate::ConfigAddon> = addons.iter().filter(|a| a.active).map(|a| crate::ConfigAddon {
                        id: SharedString::from(a.base.id.to_string()),
                        name: SharedString::from(a.name.as_str()),
                        price: a.price.to_f64().unwrap_or(0.0) as f32,
                        price_display: SharedString::from(format_addon_price(a.price.to_f64().unwrap_or(0.0))),
                        selected: false,
                    }).collect();
                    crate::ConfigAddonGroup {
                        id: SharedString::from(g.base.id.to_string()),
                        name: SharedString::from(g.name.as_str()),
                        selection: SharedString::from(g.selection.as_str()),
                        hint: SharedString::from(hint),
                        min_select: g.min_select,
                        max_select: g.max_select,
                        addons: ModelRc::new(VecModel::from(items)),
                    }
                }).collect();
                ui.set_config_product_id(SharedString::from(pid_str));
                ui.set_config_product_name(SharedString::from(product_name));
                ui.set_config_base_price(base_price.to_f64().unwrap_or(0.0) as f32);
                ui.set_config_qty(1);
                ui.set_config_variations(ModelRc::new(VecModel::from(vars_ui)));
                ui.set_config_addon_groups(ModelRc::new(VecModel::from(groups_ui)));
                ui.set_config_error(SharedString::default());
                recompute_config_total(&ui);
            });
        });
    });
}

/// Reabre o configurador para um item JÁ presente em `edit_order_items`,
/// reaproveitando a lógica de `start_product_config` mas com:
///   - `config_edit_idx` setado para o `idx`, para que o
///     `config_confirm` SUBSTITUA `items[idx]` em vez de fazer push;
///   - as opções/adicionais pré-marcados a partir do `addons_json`
///     salvo no item (matching por `(group, name)` quando o JSON é do
///     schema novo; fallback por `name` apenas para itens gravados
///     antes da introdução do campo `group`);
///   - a `config_qty` já preenchida com a qty do item.
///
/// Se o produto não tem variações nem adicionais, o item é
/// inalterável aqui (só qty via +/−) — mostra toast e não abre.
pub(crate) fn setup_edit_order_edit_item(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_edit_order_edit_item(move |idx: i32| {
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        let items_model = ui_ref.get_edit_order_items();
        let Some(item) = items_model.row_data(idx as usize) else { return };
        let pid_str = item.product_id.to_string();
        let Ok(pid_uuid) = Uuid::parse_str(&pid_str) else { return };
        let qty_i: i32 = item.qty.parse::<f64>().unwrap_or(1.0).max(1.0) as i32;
        let addons_json_raw = item.addons_json.to_string();

        // Pares (group, name) pré-selecionados. Vazio quando o item
        // não tem snapshot. Tolera JSON antigo: `group` pode ser ""
        // e o matching cai em "qualquer grupo" (fallback por nome).
        let selected_pairs: Vec<(String, String)> = if addons_json_raw.is_empty() {
            Vec::new()
        } else {
            serde_json::from_str::<serde_json::Value>(&addons_json_raw)
                .ok()
                .and_then(|v| v.as_array().cloned())
                .map(|arr| arr.into_iter().filter_map(|v| {
                    let name = v.get("name").and_then(|x| x.as_str())?.to_string();
                    let group = v.get("group").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    Some((group, name))
                }).collect())
                .unwrap_or_default()
        };

        let state2 = state.clone();
        let ui_weak2 = ui_weak.clone();
        handle.spawn(async move {
            let cid = state2.company_id();
            let product = match state2.product_service.find_by_id(cid, pid_uuid).await {
                Ok(Some(p)) => p,
                _ => return,
            };
            let has_variations = product.variations.as_deref()
                .map(|s| !s.trim().is_empty() && s.trim() != "[]")
                .unwrap_or(false);
            let has_addons = !product.addon_group_ids.is_empty();
            if !has_variations && !has_addons {
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak2.upgrade() {
                        show_toast(&ui, "Produto sem opções configuráveis", "info");
                    }
                });
                return;
            }
            // Mesmas etapas do `start_product_config`: carrega grupos
            // de adicionais e parseia variações.
            let mut groups: Vec<(letaf_core::addon_group::model::AddonGroup, Vec<letaf_core::addon::model::Addon>)> = Vec::new();
            for gid in &product.addon_group_ids {
                if let Ok(Some(g)) = state2.addon_group_service.find_by_id(cid, *gid).await {
                    let addons = state2.addon_service.find_by_group(cid, *gid).await.unwrap_or_default();
                    groups.push((g, addons));
                }
            }
            let variations_parsed: Vec<VariationJson> = product.variations
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default();
            // Desconto eventual já aplicado (fixed/percent/bulk com qty=1).
            // Mesma lógica do server em `verify_item_prices` — evita
            // "Price mismatch" no sync.
            let base_price = letaf_core::discount::effective_unit_price(&product, 1.0);
            let product_name = product.name.clone();

            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak2.upgrade() else { return };
                // Helper de matching: marca selected quando há match
                // exato por (group, name); aceita group vazio como
                // wildcard para tolerar JSON antigo. NÃO faz match por
                // nome em grupo diferente — evita falso-positivo entre
                // variações e adicionais com o mesmo rótulo. Definido
                // dentro do `invoke_from_event_loop` para que o move
                // dos pares ocorra junto com `variations_parsed` /
                // `groups` (a closure outer já é 'static).
                let is_selected = |group_title: &str, opt_name: &str| -> bool {
                    selected_pairs.iter().any(|(g, n)| {
                        n == opt_name && (g.is_empty() || g == group_title)
                    })
                };
                let vars_ui: Vec<crate::ConfigVariation> = variations_parsed.into_iter().map(|v| {
                    let hint = variation_hint(&v);
                    let title = v.title.clone();
                    let opts: Vec<crate::ConfigOption> = v.options.iter().map(|o| crate::ConfigOption {
                        name: SharedString::from(o.name.as_str()),
                        price: o.price as f32,
                        price_display: SharedString::from(format_addon_price(o.price)),
                        selected: is_selected(&title, &o.name),
                    }).collect();
                    crate::ConfigVariation {
                        title: SharedString::from(title),
                        selection: SharedString::from(v.selection.clone()),
                        hint: SharedString::from(hint),
                        required: v.required,
                        min_select: v.min_select as i32,
                        max_select: v.max_select as i32,
                        options: ModelRc::new(VecModel::from(opts)),
                    }
                }).collect();
                let groups_ui: Vec<crate::ConfigAddonGroup> = groups.iter().map(|(g, addons)| {
                    let hint = addon_group_hint(g);
                    let group_name = g.name.to_string();
                    let items: Vec<crate::ConfigAddon> = addons.iter().filter(|a| a.active).map(|a| crate::ConfigAddon {
                        id: SharedString::from(a.base.id.to_string()),
                        name: SharedString::from(a.name.as_str()),
                        price: a.price.to_f64().unwrap_or(0.0) as f32,
                        price_display: SharedString::from(format_addon_price(a.price.to_f64().unwrap_or(0.0))),
                        selected: is_selected(&group_name, &a.name),
                    }).collect();
                    crate::ConfigAddonGroup {
                        id: SharedString::from(g.base.id.to_string()),
                        name: SharedString::from(group_name),
                        selection: SharedString::from(g.selection.as_str()),
                        hint: SharedString::from(hint),
                        min_select: g.min_select,
                        max_select: g.max_select,
                        addons: ModelRc::new(VecModel::from(items)),
                    }
                }).collect();
                ui.set_config_product_id(SharedString::from(pid_str));
                ui.set_config_product_name(SharedString::from(product_name));
                ui.set_config_base_price(base_price.to_f64().unwrap_or(0.0) as f32);
                ui.set_config_qty(qty_i.max(1));
                ui.set_config_variations(ModelRc::new(VecModel::from(vars_ui)));
                ui.set_config_addon_groups(ModelRc::new(VecModel::from(groups_ui)));
                ui.set_config_error(SharedString::default());
                ui.set_config_edit_idx(idx);
                recompute_config_total(&ui);
            });
        });
    });
}

/// Toggle de opção em variação. Para "single", desmarca os irmãos.
/// Para "multi"/"max_value" com `max_select > 0`: refuso o toggle se
/// já atingiu o limite (operador deve desmarcar outra antes).
pub(crate) fn setup_config_toggle_variation(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_config_toggle_variation(move |vidx, oidx| {
        let Some(ui) = ui_weak.upgrade() else { return };
        let variations = ui.get_config_variations();
        let Some(v) = variations.row_data(vidx as usize) else { return };
        let opts = v.options.clone();
        let selection = v.selection.to_string();
        let max_select = v.max_select as usize;
        let opt_count = opts.row_count();
        let Some(target) = opts.row_data(oidx as usize) else { return };
        let new_state = !target.selected;
        // Enforcement: marcar nova opção em modo multi/max_value
        // respeitando `max_select`.
        if new_state && selection != "single" && max_select > 0 {
            let already = (0..opt_count)
                .filter_map(|i| opts.row_data(i))
                .filter(|o| o.selected)
                .count();
            if already >= max_select {
                ui.set_config_error(SharedString::from(format!(
                    "“{}”: limite de {} opções atingido", v.title, max_select
                )));
                return;
            }
        }
        for i in 0..opt_count {
            if let Some(mut o) = opts.row_data(i) {
                if i == oidx as usize {
                    o.selected = new_state;
                } else if selection == "single" && new_state {
                    // "single": ao marcar um, desmarca os outros.
                    o.selected = false;
                }
                opts.set_row_data(i, o);
            }
        }
        if let Some(vm) = variations.as_any().downcast_ref::<VecModel<crate::ConfigVariation>>() {
            vm.set_row_data(vidx as usize, crate::ConfigVariation {
                title: v.title.clone(),
                selection: v.selection.clone(),
                hint: v.hint.clone(),
                required: v.required,
                min_select: v.min_select,
                max_select: v.max_select,
                options: opts,
            });
        }
        ui.set_config_error(SharedString::default());
        recompute_config_total(&ui);
    });
}

/// Toggle de adicional. Para "single", desmarca os irmãos do grupo.
/// Para "multi" com `max_select > 0`: refuso ao atingir o limite.
pub(crate) fn setup_config_toggle_addon(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_config_toggle_addon(move |gidx, aidx| {
        let Some(ui) = ui_weak.upgrade() else { return };
        let groups = ui.get_config_addon_groups();
        let Some(g) = groups.row_data(gidx as usize) else { return };
        let addons = g.addons.clone();
        let selection = g.selection.to_string();
        let max_select = g.max_select as usize;
        let count = addons.row_count();
        let Some(target) = addons.row_data(aidx as usize) else { return };
        let new_state = !target.selected;
        if new_state && selection != "single" && max_select > 0 {
            let already = (0..count)
                .filter_map(|i| addons.row_data(i))
                .filter(|a| a.selected)
                .count();
            if already >= max_select {
                ui.set_config_error(SharedString::from(format!(
                    "“{}”: limite de {} opções atingido", g.name, max_select
                )));
                return;
            }
        }
        for i in 0..count {
            if let Some(mut a) = addons.row_data(i) {
                if i == aidx as usize {
                    a.selected = new_state;
                } else if selection == "single" && new_state {
                    a.selected = false;
                }
                addons.set_row_data(i, a);
            }
        }
        if let Some(vm) = groups.as_any().downcast_ref::<VecModel<crate::ConfigAddonGroup>>() {
            vm.set_row_data(gidx as usize, crate::ConfigAddonGroup {
                id: g.id.clone(),
                name: g.name.clone(),
                selection: g.selection.clone(),
                hint: g.hint.clone(),
                min_select: g.min_select,
                max_select: g.max_select,
                addons,
            });
        }
        ui.set_config_error(SharedString::default());
        recompute_config_total(&ui);
    });
}

pub(crate) fn setup_config_inc_qty(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_config_inc_qty(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        ui.set_config_qty(ui.get_config_qty() + 1);
        recompute_config_total(&ui);
    });
}

pub(crate) fn setup_config_dec_qty(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_config_dec_qty(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        let cur = ui.get_config_qty();
        if cur > 1 {
            ui.set_config_qty(cur - 1);
            recompute_config_total(&ui);
        }
    });
}

/// Confirma a configuração: valida required/min/max, monta o
/// snapshot, empurra `EditOrderItem` na lista do modal e limpa o
/// estado do configurador. Se a validação falhar, escreve a mensagem
/// em `config-error` e retorna sem empurrar.
pub(crate) fn setup_config_confirm(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_config_confirm_add(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        let pid = ui.get_config_product_id().to_string();
        if pid.is_empty() { return; }
        // ── Validação prévia (espelha o que o backend exige) ──
        let variations = ui.get_config_variations();
        for vi in 0..variations.row_count() {
            let v = variations.row_data(vi).unwrap();
            let selected = (0..v.options.row_count())
                .filter_map(|i| v.options.row_data(i))
                .filter(|o| o.selected)
                .count() as i32;
            let min_required = if v.required && v.min_select <= 0 { 1 } else { v.min_select };
            if min_required > 0 && selected < min_required {
                let msg = if v.required && min_required == 1 {
                    format!("“{}”: selecione pelo menos uma opção", v.title)
                } else {
                    format!("“{}”: selecione pelo menos {} opções", v.title, min_required)
                };
                ui.set_config_error(SharedString::from(msg));
                return;
            }
        }
        let addon_groups_validate = ui.get_config_addon_groups();
        for gi in 0..addon_groups_validate.row_count() {
            let g = addon_groups_validate.row_data(gi).unwrap();
            if g.min_select <= 0 { continue; }
            let selected = (0..g.addons.row_count())
                .filter_map(|i| g.addons.row_data(i))
                .filter(|a| a.selected)
                .count() as i32;
            if selected < g.min_select {
                ui.set_config_error(SharedString::from(format!(
                    "“{}”: selecione pelo menos {} opções", g.name, g.min_select
                )));
                return;
            }
        }
        ui.set_config_error(SharedString::default());

        let name = ui.get_config_product_name().to_string();
        let base = ui.get_config_base_price() as f64;
        let qty = ui.get_config_qty().max(1) as f64;
        // Monta o snapshot mesma forma do web (vide
        // `build_variations_snapshot` / `build_addons_snapshot`).
        // Cada item carrega `group` (título da variação / nome do grupo
        // de adicionais) para que o resumo no detalhe e nas comandas
        // possa exibir agrupado: "Sabor: A, B · Borda: C".
        let mut snapshot: Vec<serde_json::Value> = Vec::new();
        let mut extras = 0.0_f64;
        for vi in 0..variations.row_count() {
            let v = variations.row_data(vi).unwrap();
            let title = v.title.to_string();
            let opts = v.options;
            let selection = v.selection.to_string();
            if selection == "max_value" {
                // Apenas a opção de maior preço entra.
                let mut winner: Option<(String, f64)> = None;
                for oi in 0..opts.row_count() {
                    let o = opts.row_data(oi).unwrap();
                    if o.selected {
                        let p = o.price as f64;
                        if winner.as_ref().map(|(_, wp)| p > *wp).unwrap_or(true) {
                            winner = Some((o.name.to_string(), p));
                        }
                    }
                }
                if let Some((n, p)) = winner {
                    extras += p;
                    snapshot.push(serde_json::json!({ "group": title.clone(), "name": n, "price": p }));
                }
            } else {
                for oi in 0..opts.row_count() {
                    let o = opts.row_data(oi).unwrap();
                    if o.selected {
                        let p = o.price as f64;
                        extras += p;
                        snapshot.push(serde_json::json!({ "group": title.clone(), "name": o.name.to_string(), "price": p }));
                    }
                }
            }
        }
        let groups = ui.get_config_addon_groups();
        for gi in 0..groups.row_count() {
            let g = groups.row_data(gi).unwrap();
            let group_name = g.name.to_string();
            let addons = g.addons;
            for ai in 0..addons.row_count() {
                let a = addons.row_data(ai).unwrap();
                if a.selected {
                    let p = a.price as f64;
                    extras += p;
                    snapshot.push(serde_json::json!({ "group": group_name.clone(), "name": a.name.to_string(), "price": p }));
                }
            }
        }
        let unit = base + extras;
        let addons_json = if snapshot.is_empty() {
            String::new()
        } else {
            serde_json::to_string(&serde_json::Value::Array(snapshot)).unwrap_or_default()
        };
        // Dois modos:
        // - `config_edit_idx < 0` → ADIÇÃO. Empurra um EditOrderItem
        //   novo com sentinel "new:<pid>" (o save gera UUID).
        // - `config_edit_idx >= 0` → EDIÇÃO. SUBSTITUI items[idx]
        //   preservando o `item_id` original (UUID existente do
        //   OrderItem ou sentinel "new:" se o item ainda nem foi
        //   salvo), para que o save atualize a mesma linha.
        let edit_idx = ui.get_config_edit_idx();
        let model = ui.get_edit_order_items();
        if let Some(vm) = model.as_any().downcast_ref::<VecModel<crate::EditOrderItem>>() {
            if edit_idx >= 0 {
                let existing = vm.row_data(edit_idx as usize);
                let item_id = existing
                    .as_ref()
                    .map(|r| r.item_id.clone())
                    .unwrap_or_else(|| SharedString::from(format!("new:{}", pid)));
                vm.set_row_data(edit_idx as usize, crate::EditOrderItem {
                    item_id,
                    product_id: SharedString::from(pid.to_string()),
                    product_name: SharedString::from(name),
                    qty: SharedString::from(format_qty(qty)),
                    unit_price: unit as f32,
                    line_total_display: SharedString::from(format!("R$ {:.2}", unit * qty)),
                    addons_json: SharedString::from(addons_json),
                });
            } else {
                vm.push(crate::EditOrderItem {
                    item_id: SharedString::from(format!("new:{}", pid)),
                    product_id: SharedString::from(pid.to_string()),
                    product_name: SharedString::from(name),
                    qty: SharedString::from(format_qty(qty)),
                    unit_price: unit as f32,
                    line_total_display: SharedString::from(format!("R$ {:.2}", unit * qty)),
                    addons_json: SharedString::from(addons_json),
                });
            }
        }
        // Limpa o configurador (e o modo edição).
        reset_config_state(&ui);
    });
}

/// "Cancelar" no configurador — limpa estado sem empurrar nada.
/// O item em edição (se houver) permanece intacto na lista.
pub(crate) fn setup_config_cancel(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_config_cancel(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        reset_config_state(&ui);
    });
}

/// Reseta TODO o estado do configurador para o ponto neutro
/// (fechado, sem produto, sem modo de edição). Centralizado em
/// um helper porque `config_confirm` e `config_cancel` precisam do
/// mesmo cleanup — manter em sincronia evita resíduo de seleções
/// vazando para o próximo produto configurado.
fn reset_config_state(ui: &MainWindow) {
    ui.set_config_product_id(SharedString::default());
    ui.set_config_product_name(SharedString::default());
    ui.set_config_variations(ModelRc::new(VecModel::from(Vec::<crate::ConfigVariation>::new())));
    ui.set_config_addon_groups(ModelRc::new(VecModel::from(Vec::<crate::ConfigAddonGroup>::new())));
    ui.set_config_qty(1);
    ui.set_config_base_price(0.0);
    ui.set_config_final_display(SharedString::from("R$ 0,00"));
    ui.set_config_error(SharedString::default());
    ui.set_config_edit_idx(-1);
}

/// Formata o snapshot `addons_json` numa string compacta para exibir
/// abaixo do nome do produto (no detalhe do pedido e nas comandas).
///
/// Schema atual: `[{"group":"Sabor","name":"Calabresa","price":2.0}, ...]`
/// → produz `"Sabor: Calabresa, Bacon · Borda: Cheddar"` (grupos
/// separados por " · ", preservando a ordem de inserção do JSON).
///
/// Schema antigo (pré-fix): `[{"name":"...","price":...}]` sem `group`
/// → cai num bucket de chave vazia formatado como lista simples
/// (`"Calabresa, Bacon"`), garantindo compatibilidade com itens
/// gravados antes desta mudança.
///
/// Devolve `""` quando JSON é vazio/ausente/inválido — caller decide
/// ocultar a linha de descrição.
pub(crate) fn format_addons_summary(addons_json: Option<&str>) -> String {
    let Some(raw) = addons_json.filter(|s| !s.is_empty()) else { return String::new(); };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(raw) else { return String::new(); };
    let Some(arr) = parsed.as_array() else { return String::new(); };

    // Agrupamento preservando ordem de inserção. Usamos Vec<(k, Vec<v>)>
    // em vez de HashMap para não embaralhar Sabor/Borda/Adicionais.
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    for v in arr {
        let Some(name) = v.get("name").and_then(|n| n.as_str()) else { continue };
        let group = v.get("group").and_then(|g| g.as_str()).unwrap_or("").to_string();
        match groups.iter_mut().find(|(k, _)| k == &group) {
            Some((_, names)) => names.push(name.to_string()),
            None => groups.push((group, vec![name.to_string()])),
        }
    }

    let parts: Vec<String> = groups
        .into_iter()
        .map(|(g, names)| {
            if g.is_empty() {
                names.join(", ")
            } else {
                format!("{}: {}", g, names.join(", "))
            }
        })
        .collect();
    parts.join(" · ")
}

pub(crate) fn format_qty(q: f64) -> String {
    if (q - q.round()).abs() < f64::EPSILON {
        format!("{}", q as i64)
    } else {
        format!("{q:.2}")
    }
}

