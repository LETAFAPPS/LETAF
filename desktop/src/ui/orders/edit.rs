use std::sync::Arc;

use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use tokio::sync::Notify;
use uuid::Uuid;

use letaf_core::order::model::{DeliveryType, Order, OrderStatus};

use crate::context::DesktopState;

use crate::MainWindow;

use super::super::helpers::show_toast;
use super::config::format_qty;

/// Callback: "Editar pedido" — carrega itens/notes/delivery do
/// pedido atual e abre o `EditOrderModal`.
///
/// Regras aplicadas (AI_RULES.md §1, §3, §11):
/// - Lê do `order_service` (não da UI) para garantir consistência
///   com o estado real do banco (a UI pode ter `detail-order` com
///   pequenas variações de formatação).
/// - Pedidos finalizados não permitem edição — toast informativo.
pub(crate) fn setup_edit_order(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_edit_order(move |id_str| {
        let id = match Uuid::parse_str(id_str.as_str()) {
            Ok(v) => v, Err(_) => return,
        };
        let state2 = state.clone();
        let ui_weak2 = ui_weak.clone();
        handle.spawn(async move {
            let cid = state2.company_id();
            let order = match state2.order_service.find_by_id(cid, id).await {
                Ok(Some(o)) => o,
                _ => {
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak2.upgrade() {
                            show_toast(&ui, "Pedido não encontrado", "error");
                        }
                    });
                    return;
                }
            };
            if matches!(order.status, OrderStatus::Delivered | OrderStatus::Cancelled) {
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak2.upgrade() {
                        show_toast(&ui, "Pedido finalizado não pode ser editado", "error");
                    }
                });
                return;
            }
            let items: Vec<crate::EditOrderItem> = order.items.iter().map(|it| {
                crate::EditOrderItem {
                    item_id: SharedString::from(it.base.id.to_string()),
                    product_id: SharedString::from(it.product_id.to_string()),
                    product_name: SharedString::from(it.product_name.as_str()),
                    qty: SharedString::from(format_qty(it.quantity)),
                    unit_price: it.unit_price as f32,
                    line_total_display: SharedString::from(format!("R$ {:.2}", it.subtotal)),
                    addons_json: SharedString::from(it.addons_json.as_deref().unwrap_or("")),
                }
            }).collect();
            let notes_clean = order.notes.clone()
                .map(|n| strip_address_prefix(&n))
                .unwrap_or_default();
            let dt = order.delivery_type.to_string();
            // Carrega produtos pro picker direto do service — o
            // modelo `ui.products` só é populado quando o operador
            // entra na aba Produtos (lazy). Aqui passamos tuplas
            // (id, name, price_str, price_display) — TUDO Send —
            // para o event loop, que reconstrói os `ProductData`
            // (não-Send porque contém `slint::Image`).
            let picker_tuples: Vec<(String, String, String, String)> = state2.product_service
                .find_all(cid).await.ok().unwrap_or_default()
                .into_iter()
                .filter(|p| p.active)
                .map(|p| (
                    p.base.id.to_string(),
                    p.name.clone(),
                    p.price.map(|v| format!("{v}")).unwrap_or_default(),
                    p.price.map(|v| format!("R$ {:.2}", v))
                        .unwrap_or_else(|| "".into()),
                ))
                .collect();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak2.upgrade() {
                    ui.set_edit_order_items(ModelRc::new(VecModel::from(items)));
                    ui.set_edit_order_notes(SharedString::from(notes_clean));
                    ui.set_edit_order_delivery_type(SharedString::from(dt));
                    ui.set_edit_order_error(SharedString::default());
                    let picker_products: Vec<crate::ProductData> = picker_tuples
                        .into_iter()
                        .map(|(id, name, price, price_display)| crate::ProductData {
                            id: SharedString::from(id),
                            name: SharedString::from(name),
                            price: SharedString::from(price),
                            price_display: SharedString::from(price_display),
                            ..Default::default()
                        })
                        .collect();
                    // O master é a fonte de verdade do filtro; o
                    // `picker-products` começa idêntico (sem filtro).
                    ui.set_edit_order_picker_master(ModelRc::new(VecModel::from(picker_products.clone())));
                    ui.set_edit_order_picker_products(ModelRc::new(VecModel::from(picker_products)));
                    ui.set_show_edit_order_modal(true);
                }
            });
        });
    });
}

/// Refiltra a lista de produtos do picker do `EditOrderModal` por
/// substring (case-insensitive). `query` vazia = mostra todos.
///
/// Filtra a partir do `edit-order-picker-master` (populado na
/// abertura do modal direto do `product_service`), NÃO do
/// `ui.products` (que é lazy e pode estar vazio).
pub(crate) fn setup_edit_order_filter_picker(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_edit_order_filter_picker(move |query| {
        let Some(ui) = ui_weak.upgrade() else { return };
        let q = query.to_string().to_lowercase();
        let master = ui.get_edit_order_picker_master();
        let mut filtered = Vec::new();
        for i in 0..master.row_count() {
            if let Some(p) = master.row_data(i) {
                if q.is_empty() || p.name.to_lowercase().contains(&q) {
                    filtered.push(p);
                }
            }
        }
        ui.set_edit_order_picker_products(ModelRc::new(VecModel::from(filtered)));
    });
}

/// Limpa o prefixo `[Tipo] Rua, Número, Bairro |` do `notes` quando
/// presente — esse formato vem do web para pedidos de entrega. No
/// editor, queremos só a observação livre do operador (sem o
/// endereço embutido).
pub(crate) fn strip_address_prefix(raw: &str) -> String {
    if raw.starts_with('[') {
        if let Some(pipe) = raw.find(" | ") {
            return raw[pipe + 3..].to_string();
        }
        return String::new();
    }
    raw.to_string()
}

/// `+ 1` na qty do item `idx`.
pub(crate) fn setup_edit_order_inc(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_edit_order_inc(move |idx| {
        let Some(ui) = ui_weak.upgrade() else { return };
        let model = ui.get_edit_order_items();
        let Some(vm) = model.as_any().downcast_ref::<VecModel<crate::EditOrderItem>>() else { return };
        let Some(mut row) = vm.row_data(idx as usize) else { return };
        let cur: f64 = row.qty.parse().unwrap_or(0.0);
        let new = cur + 1.0;
        row.qty = SharedString::from(format_qty(new));
        row.line_total_display = SharedString::from(format!("R$ {:.2}", new * row.unit_price as f64));
        vm.set_row_data(idx as usize, row);
    });
}

/// `− 1` na qty; quando chega a 0, remove a linha.
pub(crate) fn setup_edit_order_dec(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_edit_order_dec(move |idx| {
        let Some(ui) = ui_weak.upgrade() else { return };
        let model = ui.get_edit_order_items();
        let Some(vm) = model.as_any().downcast_ref::<VecModel<crate::EditOrderItem>>() else { return };
        let Some(mut row) = vm.row_data(idx as usize) else { return };
        let cur: f64 = row.qty.parse().unwrap_or(0.0);
        let new = cur - 1.0;
        if new <= 0.0 {
            vm.remove(idx as usize);
            return;
        }
        row.qty = SharedString::from(format_qty(new));
        row.line_total_display = SharedString::from(format!("R$ {:.2}", new * row.unit_price as f64));
        vm.set_row_data(idx as usize, row);
    });
}

/// Remove o item no índice `idx` do model do modal de edição.
pub(crate) fn setup_edit_order_delete(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_edit_order_delete(move |idx| {
        let Some(ui) = ui_weak.upgrade() else { return };
        let model = ui.get_edit_order_items();
        let Some(vm) = model.as_any().downcast_ref::<VecModel<crate::EditOrderItem>>() else { return };
        if (idx as usize) < vm.row_count() {
            vm.remove(idx as usize);
        }
    });
}

/// Adiciona um produto à lista de itens em edição. `item_id` vazio
/// serve de sentinel "novo" — o save vai gerar UUID no service.
/// Lê `name` e `price` direto do products model (Slint) já em memória.
pub(crate) fn setup_edit_order_add_product(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_edit_order_add_product(move |pid| {
        let Some(ui) = ui_weak.upgrade() else { return };
        // Resolve o produto na lista mestra do picker (carregada do
        // service no `setup_edit_order` — não depende de a aba
        // Produtos ter sido aberta).
        let master = ui.get_edit_order_picker_master();
        let mut found: Option<(String, f64)> = None;
        for i in 0..master.row_count() {
            if let Some(p) = master.row_data(i) {
                if p.id == pid {
                    let price: f64 = p.price.parse().unwrap_or(0.0);
                    found = Some((p.name.to_string(), price));
                    break;
                }
            }
        }
        let (name, unit) = match found {
            Some(v) => v,
            None => { show_toast(&ui, "Produto não encontrado", "error"); return; }
        };
        let model = ui.get_edit_order_items();
        let Some(vm) = model.as_any().downcast_ref::<VecModel<crate::EditOrderItem>>() else { return };
        // Se já existe linha do MESMO produto (pelo nome), apenas
        // incrementa qty — evita duplicar linhas do mesmo produto.
        for i in 0..vm.row_count() {
            if let Some(mut row) = vm.row_data(i) {
                if row.product_name == name {
                    let cur: f64 = row.qty.parse().unwrap_or(0.0);
                    let new = cur + 1.0;
                    row.qty = SharedString::from(format_qty(new));
                    row.line_total_display = SharedString::from(format!("R$ {:.2}", new * row.unit_price as f64));
                    vm.set_row_data(i, row);
                    return;
                }
            }
        }
        vm.push(crate::EditOrderItem {
            // `item-id` com sentinel "new:<pid>" sinaliza ao save que
            // o item ainda não existe no banco — o service gera UUID
            // novo. Mantemos o sentinel mesmo após adicionar
            // `product-id` à struct: o save usa `item-id` como flag
            // de "novo vs existente" via `strip_prefix("new:")`.
            item_id: SharedString::from(format!("new:{}", pid)),
            product_id: SharedString::from(pid.to_string()),
            product_name: SharedString::from(name),
            qty: SharedString::from("1"),
            unit_price: unit as f32,
            line_total_display: SharedString::from(format!("R$ {:.2}", unit)),
            addons_json: SharedString::default(),
        });
    });
}

/// "Salvar alterações" — lê o modal e chama `update_basics` com a
/// lista FINAL de itens (existentes + novos). Resolve product_ids e
/// gera novos UUIDs no service.
pub(crate) fn setup_save_edit_order(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_save_edit_order(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        let order_id_str = ui_ref.get_detail_order().id.to_string();
        let id = match Uuid::parse_str(&order_id_str) {
            Ok(v) => v, Err(_) => return,
        };
        // Coleta a lista FINAL do modal. Para cada linha, monta
        // OrderItem (existente: id já parsado; novo: id = nil para o
        // service gerar). product_id vem do snapshot original (ou do
        // sentinel "new:<uuid>" para novos).
        let items_model = ui_ref.get_edit_order_items();
        // (item_id, product_name, qty, unit_price, addons_json)
        let mut rows: Vec<(String, String, f64, f64, String)> = Vec::new();
        for i in 0..items_model.row_count() {
            if let Some(row) = items_model.row_data(i) {
                let qty: f64 = row.qty.parse().unwrap_or(0.0);
                rows.push((
                    row.item_id.to_string(),
                    row.product_name.to_string(),
                    qty,
                    row.unit_price as f64,
                    row.addons_json.to_string(),
                ));
            }
        }
        let notes_raw = ui_ref.get_edit_order_notes().to_string();
        // `delivery_type` não é mais editável no modal (UX simplificado:
        // alterar entre entrega/retirada no meio do fluxo é raro e
        // afeta endereço/taxa — o operador cancela e refaz o pedido
        // se precisar). Mantém o tipo original.
        let ui_weak2 = ui_ref.as_weak();
        let state2 = state.clone();
        let notify = sync_notify.clone();
        handle.spawn(async move {
            let cid = state2.company_id();
            let prior = state2.order_service.find_by_id(cid, id).await.ok().flatten();
            let final_notes = build_notes_keeping_address(prior.as_ref(), &notes_raw);
            // Preserva o tipo de entrega original (não é editável no
            // modal). Default `Delivery` só serve de fallback caso o
            // pedido não tenha sido carregado (extremamente raro).
            let delivery_type = prior.as_ref()
                .map(|o| o.delivery_type.clone())
                .unwrap_or(DeliveryType::Delivery);
            // Hidrata OrderItems: existentes (item_id == uuid) carregam
            // o product_id/addons_json/notes do snapshot original;
            // novos (item_id começa com "new:<product_id>") apenas
            // marcam id=nil.
            let prior_items_by_id: std::collections::HashMap<Uuid, _> = prior
                .as_ref()
                .map(|o| o.items.iter().map(|i| (i.base.id, i.clone())).collect())
                .unwrap_or_default();
            let mut final_items: Vec<letaf_core::order::model::OrderItem> = Vec::new();
            for (item_id, name, qty, unit, addons_json) in rows {
                if let Some(rest) = item_id.strip_prefix("new:") {
                    // Item novo (via picker direto ou via configurador).
                    // O `addons_json` da row é o snapshot quando vem do
                    // configurador; vazio para adds simples.
                    let pid = Uuid::parse_str(rest).unwrap_or(Uuid::nil());
                    let aj = if addons_json.is_empty() { None } else { Some(addons_json) };
                    final_items.push(letaf_core::order::model::OrderItem {
                        base: letaf_core::entity::BaseFields {
                            id: Uuid::nil(), // service gera
                            company_id: cid,
                            created_at: chrono::Utc::now().naive_utc(),
                            updated_at: chrono::Utc::now().naive_utc(),
                            deleted_at: None,
                            synced: false,
                        },
                        order_id: id,
                        product_id: pid,
                        product_name: name,
                        quantity: qty,
                        unit_price: unit,
                        subtotal: 0.0, // service recalcula
                        notes: None,
                        addons_json: aj,
                    });
                    continue;
                }
                let iid = match Uuid::parse_str(&item_id) { Ok(v) => v, Err(_) => continue };
                if let Some(orig) = prior_items_by_id.get(&iid) {
                    let mut it = orig.clone();
                    it.quantity = qty;
                    it.product_name = name;
                    it.unit_price = unit;
                    // Propaga o snapshot REconfigurado via "Editar item":
                    // sem isso, o save preservaria o `addons_json`
                    // original mesmo quando o operador trocou
                    // sabor/borda/adicionais. Vazio → None (item
                    // perdeu todas as configurações).
                    it.addons_json = if addons_json.is_empty() { None } else { Some(addons_json) };
                    final_items.push(it);
                }
            }
            let result = state2.order_service.update_basics(
                cid, id, final_items, final_notes, delivery_type
            ).await;
            if result.is_ok() { notify.notify_one(); }
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak2.upgrade() else { return };
                match result {
                    Ok(_) => {
                        ui.set_show_edit_order_modal(false);
                        ui.set_edit_order_error(SharedString::default());
                        show_toast(&ui, "Pedido Atualizado", "success");
                        ui.invoke_open_order(ui.get_detail_order().id);
                        ui.invoke_refresh_orders();
                    }
                    Err(e) => {
                        ui.set_edit_order_error(SharedString::from(format!("{e}")));
                    }
                }
            });
        });
    });
}

/// Mescla o `notes` puro digitado pelo operador com o prefixo de
/// endereço (`[Label] Rua, Nº, Bairro | ...`) que vem do web. O
/// prefixo é preservado para pedidos de entrega vindos do cardápio
/// online — sem isso, salvar perderia o endereço.
fn build_notes_keeping_address(prior: Option<&Order>, new_clean: &str) -> Option<String> {
    let clean = new_clean.trim();
    let prefix = prior
        .and_then(|o| o.notes.as_deref())
        .filter(|s| s.starts_with('['))
        .map(|s| {
            if let Some(pipe) = s.find(" | ") {
                s[..pipe].to_string()
            } else {
                s.to_string()
            }
        });
    match (prefix, clean.is_empty()) {
        (Some(p), true) => Some(p),
        (Some(p), false) => Some(format!("{p} | {clean}")),
        (None, true) => None,
        (None, false) => Some(clean.to_string()),
    }
}

