
use slint::{Model, ModelRc, SharedString, VecModel};
use uuid::Uuid;

use letaf_core::product::model::BalanceMode;

use crate::{MainWindow, VariationData, VariationOptionData};

use super::state::ui_to_availability_json;
use super::editors::ui_tiers_to_json;

/// Dados do formulário de produto lidos da UI.
pub(crate) struct ProductFormData {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) price: Option<f64>,
    /// Preço de custo (Fase 9). `None` quando o operador deixou em branco —
    /// nesse caso `Product::margin_pct` retorna `None` e a UI mostra "—".
    pub(crate) cost_price: Option<f64>,
    pub(crate) stock_quantity: f64,
    /// Estoque mínimo desejado (Fase 9). `0.0` desliga o alerta de
    /// estoque baixo / a sugestão de compra.
    pub(crate) min_stock: f64,
    pub(crate) unlimited_stock: bool,
    pub(crate) availability_schedule: Option<String>,
    pub(crate) discount_kind: Option<String>,
    pub(crate) discount_value: Option<f64>,
    pub(crate) discount_min_qty: Option<f64>,
    pub(crate) discount_tiers: Option<String>,
    pub(crate) barcode: Option<String>,
    pub(crate) unit: String,
    pub(crate) balance_mode: BalanceMode,
    pub(crate) image_data: Option<String>,
    pub(crate) cover_color: Option<String>,
    pub(crate) category_id: Option<Uuid>,
    pub(crate) subcategory_id: Option<Uuid>,
    pub(crate) addon_group_ids: Vec<Uuid>,
    /// JSON cru das variações (Fase 5). `None` quando não há.
    /// Fase 5B vai ligar a UI; por ora só passa adiante.
    pub(crate) variations: Option<String>,
}

/// Lê campos do formulário de produto da UI.
///
/// Regras aplicadas (AI_RULES.md §11):
/// - Normaliza `stock_quantity` conforme a unidade: `un`/`cx` arredondam para
///   inteiro; `kg` mantém decimais.
/// - Aceita tanto `,` quanto `.` como separador decimal (locale pt-BR vs
///   formato técnico). Internamente o número segue o padrão Rust/JSON com `.`.
pub(crate) fn read_product_form(ui: &MainWindow) -> ProductFormData {
    let desc = ui.get_product_description().to_string();
    let barcode_str = ui.get_product_barcode().to_string();
    let unit_raw = ui.get_product_unit().to_string();
    let unit = if unit_raw.is_empty() { "un".to_string() } else { unit_raw };
    let img = ui.get_product_image_data().to_string();
    let cover_str = ui.get_product_cover_color().to_string();
    let cat_str = ui.get_product_category_id().to_string();
    let sub_str = ui.get_product_subcategory_id().to_string();
    let stock_raw: f64 = parse_decimal(&ui.get_product_stock_quantity()).unwrap_or(0.0);
    let stock_quantity = if unit == "kg" { stock_raw } else { stock_raw.round() };
    let unlimited_stock = ui.get_product_unlimited_stock();
    let availability_enabled = ui.get_product_availability_enabled();
    let availability_model = ui.get_product_availability();
    let availability_schedule = ui_to_availability_json(availability_enabled, &availability_model);

    // Desconto: sentinel "none" do UI vira None no domínio.
    // - fixed/percent: value único (sem tiers, sem min_qty).
    // - bulk_*: tiers via `product-discount-tiers` (JSON); value/min_qty
    //   ficam None — quem manda é o array.
    let discount_kind_str = ui.get_product_discount_kind().to_string();
    let is_bulk = discount_kind_str.starts_with("bulk_");
    let (discount_kind, discount_value, discount_min_qty, discount_tiers) =
        if discount_kind_str == "none" || discount_kind_str.is_empty() {
            (None, None, None, None)
        } else if is_bulk {
            (Some(discount_kind_str), None, None, ui_tiers_to_json(ui))
        } else {
            (
                Some(discount_kind_str),
                parse_decimal(&ui.get_product_discount_value()),
                None,
                None,
            )
        };

    let balance_mode = BalanceMode::from_db_str(ui.get_product_balance_mode().as_ref())
        .unwrap_or_default();
    let has_image = !img.is_empty();
    // Custo e mínimo (Fase 9). Lê dos campos novos do MainWindow; vazio
    // = None / 0.0 (mesma semântica do preço).
    let cost_price = parse_decimal(&ui.get_product_cost_price());
    let min_stock_raw = parse_decimal(&ui.get_product_min_stock()).unwrap_or(0.0);
    // Mesmo arredondamento do estoque atual: `un`/`cx` → inteiro, `kg`
    // mantém decimais. Mantém a coerência visual entre os dois campos.
    let min_stock = if unit == "kg" { min_stock_raw.max(0.0) } else { min_stock_raw.round().max(0.0) };
    ProductFormData {
        name: ui.get_product_name().to_string(),
        description: if desc.is_empty() { None } else { Some(desc) },
        price: parse_decimal(&ui.get_product_price()),
        cost_price,
        stock_quantity,
        min_stock,
        unlimited_stock,
        availability_schedule,
        discount_kind,
        discount_value,
        discount_min_qty,
        discount_tiers,
        barcode: if barcode_str.is_empty() { None } else { Some(barcode_str) },
        unit,
        balance_mode,
        image_data: if has_image { Some(img) } else { None },
        // `cover_color` só tem sentido junto com uma imagem; se o usuário
        // removeu a imagem (image-data vazio), zera também para não
        // persistir cor órfã.
        cover_color: if has_image && !cover_str.is_empty() { Some(cover_str) } else { None },
        category_id: if cat_str.is_empty() { None } else { Uuid::parse_str(&cat_str).ok() },
        subcategory_id: if sub_str.is_empty() { None } else { Uuid::parse_str(&sub_str).ok() },
        // Lê IDs já selecionados via UI (Fase 4B popula esta lista; aqui
        // ela vem vazia até a UI ser ligada — produto fica sem addons).
        addon_group_ids: ui_addon_group_ids(ui),
        // Fase 5B vai popular esse JSON a partir do card "Variações"
        // do form; por ora vem `None` (produto sem variações).
        variations: ui_variations_json(ui),
    }
}

/// Lê a lista mutável de variações do form Slint e serializa para o
/// JSON persistido (`Product.variations`).
///
/// Regras aplicadas (AI_RULES.md §8, §11):
/// - Descarta variações com `title` vazio e opções com `name` ou
///   `price` inválido — UI permite linhas incompletas durante a
///   edição, mas o save normaliza.
/// - Preço aceita "16,90" (pt-BR) ou "16.90"; vazia/inválida vira
///   `0.0` para evitar bloquear o save por um typo.
/// - Devolve `None` quando não restar nenhuma variação válida.
fn ui_variations_json(ui: &MainWindow) -> Option<String> {
    let model = ui.get_product_variations();
    let mut arr: Vec<serde_json::Value> = Vec::new();
    for i in 0..model.row_count() {
        let Some(v) = model.row_data(i) else { continue };
        let title = v.title.trim().to_string();
        if title.is_empty() { continue; }
        let selection = match v.selection.as_str() {
            "single" | "multi" | "max_value" => v.selection.to_string(),
            _ => "single".to_string(),
        };
        let mut opts: Vec<serde_json::Value> = Vec::new();
        for j in 0..v.options.row_count() {
            let Some(opt) = v.options.row_data(j) else { continue };
            let name = opt.name.trim().to_string();
            if name.is_empty() { continue; }
            let price = parse_decimal(&opt.price).unwrap_or(0.0).max(0.0);
            opts.push(serde_json::json!({ "name": name, "price": price }));
        }
        if opts.is_empty() { continue; }
        // Mín./máx. de seleções — só fazem sentido em multi/max_value.
        // Normaliza para um intervalo sempre válido (não bloqueia o
        // save por typo): min ∈ [0, n], max ∈ {0=sem limite} ∪ [min, n].
        let n = opts.len() as i64;
        let (min_sel, max_sel) = if selection == "multi" || selection == "max_value" {
            let mut mn = parse_uint(&v.min_select).min(n);
            let mut mx = parse_uint(&v.max_select);
            if mx > 0 {
                mx = mx.min(n);
                if mn > mx { mn = mx; }
            }
            (mn, mx)
        } else {
            (0, 0)
        };
        arr.push(serde_json::json!({
            "title": title,
            "selection": selection,
            "required": v.required,
            "min_select": min_sel,
            "max_select": max_sel,
            "options": opts,
        }));
    }
    if arr.is_empty() { return None; }
    serde_json::to_string(&serde_json::Value::Array(arr)).ok()
}

/// Parseia o JSON salvo em `Product.variations` para o VecModel
/// que a UI Slint consome. JSON inválido → lista vazia (a UI cai
/// em "Nenhuma variação cadastrada" sem travar).
pub(crate) fn parse_variations_for_ui(json: &str) -> Vec<VariationData> {
    if json.trim().is_empty() { return Vec::new(); }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let Some(arr) = value.as_array() else { return Vec::new(); };
    arr.iter().filter_map(|v| {
        let obj = v.as_object()?;
        let title = obj.get("title")?.as_str()?.to_string();
        let selection = obj.get("selection")?.as_str()?.to_string();
        let required = obj.get("required").and_then(|x| x.as_bool()).unwrap_or(false);
        let min_sel = obj.get("min_select").and_then(|x| x.as_i64()).unwrap_or(0);
        let max_sel = obj.get("max_select").and_then(|x| x.as_i64()).unwrap_or(0);
        let options_arr = obj.get("options")?.as_array()?;
        let options: Vec<VariationOptionData> = options_arr.iter().filter_map(|opt| {
            let o = opt.as_object()?;
            let name = o.get("name")?.as_str()?.to_string();
            let price = o.get("price")?.as_f64()?;
            Some(VariationOptionData {
                name: SharedString::from(name),
                price: SharedString::from(format_variation_price(price)),
            })
        }).collect();
        Some(VariationData {
            title: SharedString::from(title),
            selection: SharedString::from(selection),
            required,
            min_select: SharedString::from(int_opt_str(min_sel)),
            max_select: SharedString::from(int_opt_str(max_sel)),
            options: ModelRc::new(VecModel::from(options)),
        })
    }).collect()
}

/// `0.0` → `"0"`, `5.5` → `"5.50"`. Mesmo critério dos demais inputs
/// monetários (display amigável; o reparse aceita ambos os formatos).
fn format_variation_price(p: f64) -> String {
    if p.fract() == 0.0 { format!("{:.0}", p) } else { format!("{p:.2}") }
}

/// Inteiro não-negativo a partir de string da UI (vazio/typo → 0).
fn parse_uint(s: &SharedString) -> i64 {
    s.trim().parse::<i64>().unwrap_or(0).max(0)
}

/// 0 → "" (mostra o placeholder); senão o número como texto.
fn int_opt_str(n: i64) -> String {
    if n <= 0 { String::new() } else { n.to_string() }
}

/// Lê IDs dos grupos selecionados nos chips do card "Adicionais" do
/// form (Fase 4B). Delega ao módulo addons (helper compartilhado).
fn ui_addon_group_ids(ui: &MainWindow) -> Vec<Uuid> {
    super::super::addons::read_selected_addon_group_ids(ui)
}

/// Faz parse de um número decimal aceitando tanto `,` quanto `.` como separador.
///
/// Regras aplicadas (AI_RULES.md §11):
/// - Usuário pt-BR digita "16,90"; código serializa em "16.90".
/// - Strings vazias ou inválidas viram `None` (caller decide o default).
pub(crate) fn parse_decimal(raw: &slint::SharedString) -> Option<f64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.replace(',', ".").parse::<f64>().ok()
}

/// Limpa mensagens de erro de validação do formulário de produto.
fn clear_product_errors(ui: &MainWindow) {
    ui.set_product_error_name(SharedString::default());
    ui.set_product_error_price(SharedString::default());
    ui.set_product_error_stock(SharedString::default());
}

/// Valida campos obrigatórios do formulário de produto.
///
/// Retorna true se todos os campos obrigatórios estão preenchidos.
/// Se algum estiver vazio, define a mensagem de erro correspondente na UI.
pub(crate) fn validate_product_form(ui: &MainWindow) -> bool {
    let mut valid = true;
    clear_product_errors(ui);

    if ui.get_product_name().trim().is_empty() {
        ui.set_product_error_name(SharedString::from("Preencha o nome do produto"));
        valid = false;
    }
    if ui.get_product_price().trim().is_empty() {
        ui.set_product_error_price(SharedString::from("Preencha o preço"));
        valid = false;
    } else if ui.get_product_price().to_string().parse::<f64>().is_err() {
        ui.set_product_error_price(SharedString::from("Preço inválido"));
        valid = false;
    }
    // Estoque ilimitado: o input de quantidade é escondido e perde sentido.
    // Não validamos preenchimento nem parse — o service força `stock = 0`.
    if !ui.get_product_unlimited_stock() {
        if ui.get_product_stock_quantity().trim().is_empty() {
            ui.set_product_error_stock(SharedString::from("Preencha o estoque"));
            valid = false;
        } else if parse_decimal(&ui.get_product_stock_quantity()).is_none() {
            ui.set_product_error_stock(SharedString::from("Estoque inválido"));
            valid = false;
        }
    }

    valid
}

