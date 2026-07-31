use std::collections::HashMap;
use rust_decimal::prelude::ToPrimitive;
use std::sync::Arc;

use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use tokio::sync::Notify;
use uuid::Uuid;

use letaf_core::order::model::{DeliveryType, Order, OrderStatus};

use crate::context::DesktopState;
use crate::format::{format_order_date, format_order_time};
use chrono::NaiveDate;

use crate::{KanbanCol, MainWindow, OrderData, OrderItemData};

use super::super::helpers::{friendly_error, show_toast};
use super::calendar::parse_ymd;
use super::config::{format_addons_summary, format_qty};

pub(crate) fn setup_refresh_orders(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_refresh_orders(move || {
        let ui_weak = ui_weak.clone();
        let state = state.clone();

        handle.spawn(async move {
            let cid = state.company_id();
            let result = load_orders_with_customers(&state, cid).await;

            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                match result {
                    Ok(all) => apply_loaded_orders(&ui, all),
                    Err(e) => {
                        ui.set_status_message(SharedString::from(friendly_error(&e)));
                    }
                }
            });
        });
    });
}

/// Reaplica a lista carregada na UI: badge de ativos, busca textual,
/// colunas do Kanban e filtro de status — fonte única, usada pelo
/// refresh E pelas mutações (avançar/cancelar) para o Kanban/grade
/// atualizarem na hora. Roda no event loop (lê props da UI).
fn apply_loaded_orders(ui: &MainWindow, all: Vec<OrderData>) {
    let filter = ui.get_order_filter_status().to_string();
    let query = ui.get_order_search_query().to_string().to_lowercase();
    // Filtro por período (calendário): início/fim em "AAAA-MM-DD".
    // Vazio = sem restrição. Mesmo dia permitido (start == end);
    // só início definido → filtra exatamente esse dia.
    let d_start = parse_ymd(ui.get_order_date_start().as_ref());
    let d_end = parse_ymd(ui.get_order_date_end().as_ref()).or(d_start);

    // Ordena do primeiro para o último (nº crescente = mais antigo
    // no topo) — fila FIFO de atendimento no Kanban e na grade.
    let mut all = all;
    all.sort_by_key(|o| o.number.as_str().parse::<u64>().unwrap_or(0));

    // Badge da sidebar: ativos sobre a lista completa (OrderData da UI).
    let active_count = all
        .iter()
        .filter(|o| {
            let s = o.status.as_str();
            s != "delivered" && s != "cancelled"
        })
        .count() as i32;
    ui.set_orders_active_count(active_count);

    // Busca textual (nº/cliente) + filtro por data, combinados.
    let searched: Vec<OrderData> = all
        .into_iter()
        .filter(|o| {
            let text_ok = query.is_empty()
                || o.number.to_lowercase().contains(query.as_str())
                || o.customer_name.to_lowercase().contains(query.as_str());
            let date_ok = match (d_start, d_end) {
                (Some(a), Some(b)) => {
                    // order_date é "DD/MM/AAAA".
                    match NaiveDate::parse_from_str(o.order_date.as_str(), "%d/%m/%Y") {
                        Ok(od) => od >= a && od <= b,
                        Err(_) => false,
                    }
                }
                _ => true,
            };
            text_ok && date_ok
        })
        .collect();

    // Colunas do Kanban (5 fixas; cancelados só pelo chip).
    ui.set_kanban_cols(ModelRc::new(VecModel::from(build_kanban_cols(&searched))));

    // Grade por status (quando não é "all"/vazio).
    let mut items: Vec<OrderData> = if filter.is_empty() || filter == "all" {
        searched
    } else {
        searched
            .into_iter()
            .filter(|o| o.status.as_str() == filter)
            .collect()
    };
    // Chip "Entregue": mesma regra do Kanban — mais recente primeiro.
    if filter == "delivered" {
        items.reverse();
    }
    let count = items.len();
    ui.set_orders(ModelRc::new(VecModel::from(items)));
    ui.set_status_message(SharedString::from(format!(
        "{count} pedido(s) carregado(s)"
    )));
}

/// Conta pedidos ATIVOS (status != Entregue/Cancelado) para o badge da
/// sidebar. Fonte única (lista de Pedidos + recompute de badges).
/// Callback: registra o pedido selecionado e carrega seus itens com imagens.
///
/// Regras aplicadas (AI_RULES.md §1, §8, §14):
/// - Apenas altera propriedades de navegação e detalhe na UI
/// - Nenhuma lógica de negócio envolvida
pub(crate) fn setup_open_order(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_open_order(move |id| {
        let ui_weak2 = ui_weak.clone();
        let state2 = state.clone();
        let id_str = id.to_string();

        // Navega imediatamente usando o OrderData já em memória.
        if let Some(u) = ui_weak.upgrade() {
            let orders = u.get_orders();
            for i in 0..orders.row_count() {
                if let Some(order) = orders.row_data(i) {
                    if order.id == id {
                        u.set_detail_order(order);
                        break;
                    }
                }
            }
            u.set_detail_order_items(ModelRc::new(VecModel::from(vec![])));
            u.set_order_detail_id(id);
        }

        // Carrega itens detalhados + imagens em background e
        // ATUALIZA `detail-order` com a versão fresca do banco —
        // a versão síncrona acima usa o orders model que pode estar
        // desatualizado (típico: logo após salvar uma edição, antes
        // do `invoke_refresh_orders` async terminar).
        handle.spawn(async move {
            let cid = state2.company_id();
            let id_uuid = match Uuid::parse_str(&id_str) {
                Ok(v) => v,
                Err(_) => return,
            };
            let order = match state2.order_service.find_by_id(cid, id_uuid).await {
                Ok(Some(o)) => o,
                _ => return,
            };
            // Resolve cliente (nome + telefone) para reconstruir `OrderData`.
            // Phone vai vazio quando o cliente não tem telefone OU quando
            // o customer já foi removido.
            let customer = state2.customer_service
                .find_by_id(cid, order.customer_id).await
                .ok().flatten();
            let customer_name = customer.as_ref()
                .map(|c| c.name.clone())
                .unwrap_or_else(|| "Cliente Presencial".into());
            let customer_phone = customer.as_ref()
                .and_then(|c| c.phone.clone())
                .unwrap_or_default();
            let mut cust_map: HashMap<Uuid, (String, String)> = HashMap::new();
            cust_map.insert(order.customer_id, (customer_name, customer_phone));
            let fresh_data = to_order_data(&order, &cust_map);
            let order_items = order.items.clone();
            // Resolve as imagens direto do product_service (não depende
            // da aba Produtos ter sido aberta). Decodifica o base64 já
            // aqui (fora do event loop) — SharedPixelBuffer é Send.
            let mut buf_map: HashMap<String, slint::SharedPixelBuffer<slint::Rgba8Pixel>> = HashMap::new();
            // Busca só os produtos DESTE pedido por id (evita carregar o
            // catálogo inteiro com todas as imagens base64 a cada abertura).
            let pids: Vec<Uuid> = order_items.iter().map(|it| it.product_id).collect();
            if let Ok(products) = state2.product_service.find_by_ids(cid, &pids).await {
                for p in &products {
                    if let Some(b64) = p.image_data.as_deref().filter(|s| !s.is_empty()) {
                        if let Some(pb) = super::super::image::decode_pixel_buffer(b64) {
                            buf_map.insert(p.base.id.to_string(), pb);
                        }
                    }
                }
            }
            let _ = slint::invoke_from_event_loop(move || {
                let Some(u) = ui_weak2.upgrade() else { return };
                let items: Vec<OrderItemData> = order_items.iter().map(|item| {
                    let pid = item.product_id.to_string();
                    let img = buf_map.get(&pid).cloned()
                        .map(slint::Image::from_rgba8)
                        .unwrap_or_default();
                    // Preço de tabela riscado — só quando o snapshot da
                    // venda registra desconto de produto no item.
                    let list_price = item.list_unit_price
                        .filter(|lp| *lp > item.unit_price)
                        .map(|lp| format!("R$ {:.2}", lp))
                        .unwrap_or_default();
                    OrderItemData {
                        product_id: SharedString::from(&pid),
                        product_name: SharedString::from(item.product_name.as_str()),
                        qty_label: SharedString::from(format!("x{}", format_qty(item.quantity))),
                        price: SharedString::from(format!("R$ {:.2}", item.unit_price)),
                        list_price: SharedString::from(list_price),
                        product_image: img,
                        addons_summary: SharedString::from(
                            format_addons_summary(item.addons_json.as_deref())
                        ),
                    }
                }).collect();
                u.set_detail_order(fresh_data);
                u.set_detail_order_items(ModelRc::new(VecModel::from(items)));
            });
        });
    });
}

/// Carrega pedidos + clientes em paralelo e monta a lista de `OrderData`.
async fn load_orders_with_customers(
    state: &DesktopState,
    company_id: Uuid,
) -> Result<Vec<OrderData>, letaf_core::error::CoreError> {
    let orders = state.order_service.find_all(company_id).await?;
    let customers = state.customer_service.find_all(company_id).await?;
    let map: HashMap<Uuid, (String, String)> = customers
        .into_iter()
        .map(|c| (c.base.id, (c.name, c.phone.unwrap_or_default())))
        .collect();
    Ok(orders.iter().map(|o| to_order_data(o, &map)).collect())
}

/// Callback: avança o status do pedido para o próximo do fluxo.
/// `order-set-payment` — registra forma de pagamento e/ou Pago/Não pago
/// no detalhe do pedido. Offline-first: grava local (synced=false),
/// atualiza o detalhe na hora e cutuca o sync (§7).
pub(crate) fn setup_set_order_payment(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_order_set_payment(move |id, method, paid| {
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let notify = sync_notify.clone();
        let Ok(order_id) = Uuid::parse_str(id.as_str()) else { return };
        let method_opt = {
            let m = method.to_string();
            if m.is_empty() { None } else { Some(m) }
        };

        handle.spawn(async move {
            let cid = state.company_id();
            let result = state
                .order_service
                .set_payment(cid, order_id, method_opt, paid)
                .await;
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                match result {
                    Ok(order) => {
                        // Atualiza o detalhe aberto na hora (sem esperar
                        // o refresh completo da lista).
                        let mut detail = ui.get_detail_order();
                        if detail.id.as_str() == order.base.id.to_string() {
                            detail.payment_method = SharedString::from(
                                order.payment_method.as_deref().unwrap_or_default(),
                            );
                            detail.paid = order.paid;
                            ui.set_detail_order(detail);
                        }
                        ui.invoke_refresh_orders();
                        notify.notify_one();
                        show_toast(
                            &ui,
                            if order.paid { "Pedido marcado como Pago" } else { "Pedido marcado como Não pago" },
                            "success",
                        );
                    }
                    Err(e) => {
                        show_toast(&ui, &friendly_error(&e), "error");
                    }
                }
            });
        });
    });
}

pub(crate) fn setup_advance_order_status(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_advance_order_status(move |id| {
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let notify = sync_notify.clone();
        let id_str = id.to_string();

        handle.spawn(async move {
            run_status_change(state, ui_weak, notify, &id_str).await;
        });
    });
}

/// Devolve à carteira do cliente o valor de um pedido fiado cancelado.
///
/// Também reajusta o espelho do fiado no Financeiro. Falha só loga: o
/// cancelamento é a fonte de verdade e não é desfeito pelo reflexo.
async fn refund_wallet_charge(
    state: &DesktopState,
    cid: uuid::Uuid,
    order: &letaf_core::order::model::Order,
) {
    if order.customer_id.is_nil() {
        return;
    }
    let account = match state
        .wallet_service
        .find_account_by_customer(cid, order.customer_id)
        .await
    {
        Ok(Some(a)) => a,
        _ => return,
    };
    match state
        .wallet_service
        .refund_order(cid, account.base.id, order.total, order.base.id)
        .await
    {
        Ok((account, _)) => {
            super::super::wallet::sync_fiado_to_finance(state, &account).await;
        }
        Err(e) => tracing::warn!(
            "pedido {} cancelado, mas a carteira não foi estornada: {e}",
            order.base.id
        ),
    }
}

/// Callback: cancela o pedido com motivo obrigatório.
///
/// Regras aplicadas (AI_RULES.md §1, §6, §11):
/// - recebe `(id, reason)` da UI;
/// - delega ao `OrderService::cancel`, que valida o motivo;
/// - estorna carteira e caixa quando a venda já havia sido cobrada.
pub(crate) fn setup_cancel_order(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_cancel_order(move |id, reason| {
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let notify = sync_notify.clone();
        let id_str = id.to_string();
        let reason = reason.to_string();

        handle.spawn(async move {
            run_cancel_order(state, ui_weak, notify, &id_str, &reason).await;
        });
    });
}

/// Executa o cancelamento (com motivo) e recarrega a lista.
async fn run_cancel_order(
    state: DesktopState,
    ui_weak: slint::Weak<MainWindow>,
    notify: Arc<Notify>,
    id_str: &str,
    reason: &str,
) {
    let order_id = match Uuid::parse_str(id_str) {
        Ok(u) => u,
        Err(e) => {
            tracing::warn!("Invalid order id: {e}");
            return;
        }
    };
    let cid = state.company_id();

    // Carteira: se a venda foi paga com saldo do cliente, devolve o
    // valor. Sem isso o pedido cancelado continuava cobrado — a dívida
    // ficava na carteira e no espelho do fiado no Financeiro.
    let charged = state
        .order_service
        .find_by_id(cid, order_id)
        .await
        .ok()
        .flatten()
        .filter(|o| {
            o.payment_method.as_deref() == Some("wallet")
                && o.status != letaf_core::order::model::OrderStatus::Cancelled
        });

    if let Err(e) = state.order_service.cancel(cid, order_id, reason).await {
        tracing::warn!("cancel order error: {e}");
        let msg = format!("Falha ao cancelar: {}", friendly_error(&e));
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_cancel_error(msg.into());
                ui.set_show_cancel_modal(true);
            }
        });
        return;
    }

    if let Some(order) = charged {
        refund_wallet_charge(&state, cid, &order).await;
    }
    notify.notify_one();

    let result = load_orders_with_customers(&state, cid).await;
    let _ = slint::invoke_from_event_loop(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        show_toast(&ui, "Pedido cancelado", "success");
        if let Ok(all) = result {
            // Se estamos na tela de detalhe do pedido cancelado, atualiza detail_order.
            let detail_id = ui.get_order_detail_id();
            if !detail_id.is_empty() {
                if let Some(item) = all.iter().find(|o| o.id == detail_id) {
                    ui.set_detail_order(item.clone());
                }
            }
            apply_loaded_orders(&ui, all);
        }
    });
}

/// Avança o status do pedido via service (lógica de transição centralizada no core).
///
/// Regras aplicadas (AI_RULES.md §1, §8):
/// - Delegação ao service::advance_status: sem lógica de negócio na UI.
/// - 2 queries ao invés de 3 (find + update; status atualizado em memória).
async fn run_status_change(
    state: DesktopState,
    ui_weak: slint::Weak<MainWindow>,
    notify: Arc<Notify>,
    id_str: &str,
) {
    let order_id = match Uuid::parse_str(id_str) {
        Ok(u) => u,
        Err(e) => {
            tracing::warn!("Invalid order id: {e}");
            return;
        }
    };
    let cid = state.company_id();
    if let Err(e) = state.order_service.advance_status(cid, order_id).await {
        tracing::warn!("advance_status error: {e}");
        return;
    }
    notify.notify_one();

    let result = load_orders_with_customers(&state, cid).await;
    let _ = slint::invoke_from_event_loop(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        show_toast(&ui, "Status Atualizado", "success");
        if let Ok(all) = result {
            // Se estamos na tela de detalhe, atualiza detail_order com o novo status.
            let detail_id = ui.get_order_detail_id();
            if !detail_id.is_empty() {
                if let Some(item) = all.iter().find(|o| o.id == detail_id) {
                    ui.set_detail_order(item.clone());
                }
            }
            apply_loaded_orders(&ui, all);
        }
    });
}

/// Converte `Order` do domínio para `OrderData` do Slint.
///
/// Separa o endereço de entrega (prefixo `[Label] ...`) das observações livres.
/// O web embute o endereço no campo `notes` como `[Label] Rua, Nº, Bairro | obs`.
///
/// `customers` mapeia `customer_id → (nome, telefone_formatado)`. Telefone
/// já vem formatado pelo caller (string vazia quando o cliente não tem ou
/// foi removido) — ele é mostrado ao lado do nome na comanda.
fn to_order_data(order: &Order, customers: &HashMap<Uuid, (String, String)>) -> OrderData {
    let (customer_name, customer_phone) = customers
        .get(&order.customer_id)
        .cloned()
        .unwrap_or_else(|| ("Cliente Presencial".into(), String::new()));
    // `format_phone` é tolerante a string vazia (devolve "") e à
     // entrada já formatada — reutilizamos o mesmo helper usado em
    // Clientes/Configurações para manter o padrão visual ("(11) 91234-5678").
    let customer_phone = if customer_phone.is_empty() {
        String::new()
    } else {
        crate::format::format_phone(&customer_phone)
    };

    let (delivery_label, addr_raw, clean_notes) =
        extract_address_from_notes(order.delivery_type == DeliveryType::Delivery, &order.notes);
    let (delivery_street, delivery_number, delivery_neighborhood, delivery_apartment) =
        parse_address_parts(&addr_raw);
    // Observações exibidas = só o que o CLIENTE digitou; tags de sistema
    // ([Balcão], [Entrega] + endereço/taxa, [Rateio...], [Pago...]) saem.
    let clean_notes = customer_notes(&clean_notes);

    // Detalhamento de valores (Fase 9, AI_RULES §1/§14):
    // - `subtotal` = soma dos `OrderItem.subtotal` (preço × qtd já com
    //   adicionais/variações somados). É o "antes do desconto".
    // - `discount_amount` é o desconto do cupom calculado no servidor.
    // - Taxa de entrega: embutida no `additional_amount` e detalhada na
    //   nota pelo PDV — extraída de lá para exibição. "Adicional" mostra
    //   só a parcela manual (additional − taxa), para a soma das linhas
    //   fechar com o Total.
    // - Sem valor → "*" (pedido do produto; antes era "—").
    let subtotal: f64 = order.items.iter().map(|i| i.subtotal.to_f64().unwrap_or(0.0)).sum();
    let fee = delivery_fee_from_notes(order.notes.as_deref());
    let delivery_fee_display = if order.delivery_type == DeliveryType::Delivery {
        if fee > 0.0 { format!("R$ {fee:.2}") } else { "Grátis".to_string() }
    } else {
        "*".to_string()
    };
    let additional_manual =
        (order.additional_amount.to_f64().unwrap_or(0.0) - fee).max(0.0);
    let additional_display = if additional_manual > 0.005 {
        format!("+ R$ {additional_manual:.2}")
    } else {
        "*".to_string()
    };
    let discount_display = if order.discount_amount > rust_decimal::Decimal::ZERO {
        format!("− R$ {:.2}", order.discount_amount.to_f64().unwrap_or(0.0))
    } else {
        "*".to_string()
    };
    let coupon_code_display = order
        .coupon_code
        .as_deref()
        .filter(|c| !c.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "*".to_string());
    let elapsed_display = format_elapsed_since(order.base.created_at, &order.status);

    OrderData {
        id: SharedString::from(order.base.id.to_string()),
        number: SharedString::from(format!("{:04}", order.number)),
        order_date: SharedString::from(format_order_date(order.base.created_at)),
        order_time: SharedString::from(format_order_time(order.base.created_at)),
        customer_name: SharedString::from(customer_name),
        customer_phone: SharedString::from(customer_phone),
        delivery_label: SharedString::from(delivery_label),
        delivery_street: SharedString::from(delivery_street),
        delivery_number: SharedString::from(delivery_number),
        delivery_neighborhood: SharedString::from(delivery_neighborhood),
        delivery_apartment: SharedString::from(delivery_apartment),
        total: SharedString::from(format!("R$ {:.2}", order.total)),
        coupon_summary: SharedString::from(match &order.coupon_code {
            Some(code) if !code.is_empty() => {
                format!("{code} (− R$ {:.2})", order.discount_amount)
            }
            _ => String::new(),
        }),
        status: SharedString::from(order.status.to_string()),
        status_label: SharedString::from(status_label(&order.status)),
        delivery_type: SharedString::from(order.delivery_type.to_string()),
        payment_method: SharedString::from(order.payment_method.as_deref().unwrap_or_default()),
        items_count: order.items.len() as i32,
        items_summary: SharedString::from(format_items_summary(&order.items)),
        notes: SharedString::from(clean_notes),
        subtotal_display: SharedString::from(format!("R$ {:.2}", subtotal)),
        discount_display: SharedString::from(discount_display),
        delivery_fee_display: SharedString::from(delivery_fee_display),
        additional_display: SharedString::from(additional_display),
        coupon_code_display: SharedString::from(coupon_code_display),
        paid: order.paid,
        elapsed_display: SharedString::from(elapsed_display),
        created_at_iso: SharedString::from(order.base.created_at.format("%Y-%m-%dT%H:%M:%S").to_string()),
    }
}

/// Formata o tempo decorrido entre `created_at` (UTC) e agora, no fuso
/// local — atalhos comuns em pt-BR. Para pedidos finalizados/cancelados,
/// retorna string vazia (a UI esconde o card).
///
/// Regras aplicadas (AI_RULES.md §1, §14):
/// - Lógica em Rust; a UI só renderiza o resultado.
/// - Granularidade: `< 1 min` → "agora"; `< 1 h` → "há X min";
///   `< 24 h` → "há X h Y min"; `>= 24 h` → "há X dias".
pub(crate) fn format_elapsed_since(
    created_at: chrono::NaiveDateTime,
    status: &OrderStatus,
) -> String {
    if matches!(status, OrderStatus::Delivered | OrderStatus::Cancelled) {
        return String::new();
    }
    let now = chrono::Utc::now().naive_utc();
    let delta = now.signed_duration_since(created_at);
    let secs = delta.num_seconds().max(0);
    if secs < 60 {
        return "Agora".to_string();
    }
    let mins = secs / 60;
    if mins < 60 {
        return format!("Há {mins} min");
    }
    let hours = mins / 60;
    let rem_mins = mins % 60;
    if hours < 24 {
        if rem_mins == 0 {
            return format!("Há {hours} h");
        }
        return format!("Há {hours} h {rem_mins} min");
    }
    let days = hours / 24;
    if days == 1 {
        "Há 1 dia".to_string()
    } else {
        format!("Há {days} dias")
    }
}

/// Observações exibidas ao operador = só o que o CLIENTE digitou.
/// O PDV embute tags de sistema no `notes` — `[Balcão]`, `[Entrega]`
/// seguido de endereço/taxa, `[Rateio: ...]`, `[Pago ...]` — que não
/// são observação e saem da exibição. Após `[Entrega]`/`[Balcão]` o
/// texto que segue (endereço · taxa) também é do sistema: descarta até
/// a próxima tag ou o fim.
fn customer_notes(raw: &str) -> String {
    let mut out = String::new();
    let mut rest = raw;
    while let Some(start) = rest.find('[') {
        out.push_str(&rest[..start]);
        let after = &rest[start..];
        match after.find(']') {
            Some(end) => {
                let tag = &after[1..end];
                rest = &after[end + 1..];
                if tag == "Entrega" || tag == "Balcão" {
                    match rest.find('[') {
                        Some(next) => rest = &rest[next..],
                        None => rest = "",
                    }
                }
            }
            None => {
                out.push_str(after);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out.trim().to_string()
}

/// Extrai a taxa de entrega detalhada pelo PDV na nota
/// ("· Taxa de entrega: R$ 5.00"). 0.0 = sem taxa registrada.
fn delivery_fee_from_notes(notes: Option<&str>) -> f64 {
    let Some(raw) = notes else { return 0.0 };
    let marker = "Taxa de entrega: R$ ";
    let Some(pos) = raw.find(marker) else { return 0.0 };
    let tail = &raw[pos + marker.len()..];
    let num: String = tail
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == ',')
        .collect();
    num.replace(',', ".").parse::<f64>().unwrap_or(0.0)
}

/// Extrai o endereço embutido no campo `notes` para pedidos de entrega.
///
/// Formato gerado pelo web: `[Label] Rua, Nº, Bairro` ou `[Label] Rua, Nº, Bairro | obs`.
/// Retorna `(label, endereco_limpo, observacoes_restantes)`.
fn extract_address_from_notes(is_delivery: bool, notes: &Option<String>) -> (String, String, String) {
    let raw = match notes.as_deref() {
        Some(s) if !s.is_empty() => s,
        _ => return (String::new(), String::new(), String::new()),
    };
    if is_delivery && raw.starts_with('[') {
        let (addr_raw, clean_notes) = if let Some(pipe) = raw.find(" | ") {
            (raw[..pipe].to_string(), raw[pipe + 3..].to_string())
        } else {
            (raw.to_string(), String::new())
        };
        if let Some(close) = addr_raw.find(']') {
            let label = addr_raw[1..close].to_string();
            let address = addr_raw.get(close + 2..).unwrap_or("").to_string();
            (label, address, clean_notes)
        } else {
            (String::new(), addr_raw, clean_notes)
        }
    } else {
        (String::new(), String::new(), raw.to_string())
    }
}

/// Decompõe "Rua, Número, Bairro" (e opcionalmente " - Ap. X") em campos separados.
///
/// Formato gerado pelo web: `{street}, {number}, {neighborhood}` ou
/// `{street}, {number}, {neighborhood} - Ap. {apartment}`.
/// Retorna `(street, number, neighborhood, apartment)` com strings vazias para campos ausentes.
pub(crate) fn parse_address_parts(address: &str) -> (String, String, String, String) {
    if address.is_empty() {
        return (String::new(), String::new(), String::new(), String::new());
    }
    let parts: Vec<&str> = address.splitn(3, ", ").collect();
    let street       = parts.first().unwrap_or(&"").to_string();
    let number       = parts.get(1).unwrap_or(&"").to_string();
    let nbh_apt_raw  = parts.get(2).unwrap_or(&"").to_string();
    if let Some(idx) = nbh_apt_raw.find(" - Ap. ") {
        let neighborhood = nbh_apt_raw[..idx].to_string();
        let apartment    = nbh_apt_raw[idx + 7..].to_string();
        (street, number, neighborhood, apartment)
    } else {
        (street, number, nbh_apt_raw, String::new())
    }
}

fn status_label(s: &OrderStatus) -> &'static str {
    match s {
        OrderStatus::Pending => "Pendente",
        OrderStatus::Confirmed => "Confirmado",
        OrderStatus::Preparing => "Preparando",
        OrderStatus::Ready => "Pronto",
        OrderStatus::Delivered => "Entregue",
        OrderStatus::Cancelled => "Cancelado",
    }
}

/// Monta as 5 colunas do Kanban (ordem fixa pending..delivered) já
/// com a lista de pedidos de cada status. Cancelados ficam de fora —
/// acessíveis só pelo chip "Cancelado". label/subtitle pt-BR; total
/// no mesmo formato dos cards (§1/§3 — UI sem lógica).
fn build_kanban_cols(orders: &[OrderData]) -> Vec<KanbanCol> {
    const COLS: &[(&str, &str, &str)] = &[
        ("pending", "Pendente", "Aguardando Confirmação"),
        ("confirmed", "Confirmado", "Aceito"),
        ("preparing", "Preparando", "Em Produção"),
        ("ready", "Pronto", "Aguardando Retirada"),
        ("delivered", "Entregue", "Concluído"),
    ];
    COLS.iter()
        .map(|(key, label, subtitle)| {
            let mut list: Vec<OrderData> = orders
                .iter()
                .filter(|o| o.status.as_str() == *key)
                .cloned()
                .collect();
            // Só "Entregue" inverte: mais recente primeiro. Os demais
            // mantêm o primeiro vem primeiro (FIFO).
            if *key == "delivered" {
                list.reverse();
            }
            // Contagem e total refletem TODOS os pedidos do status.
            let count = list.len() as i32;
            let total: f64 = list
                .iter()
                .map(|o| {
                    // `total` vem como "R$ 47.00" — soma o numérico.
                    o.total
                        .as_str()
                        .trim_start_matches("R$ ")
                        .trim()
                        .parse::<f64>()
                        .unwrap_or(0.0)
                })
                .sum();
            // Entregue é histórico: no kanban mostra no máximo 10 cards
            // (os mais recentes); o restante fica acessível pelo filtro
            // "Entregue" (com paginação).
            if *key == "delivered" {
                list.truncate(10);
            }
            KanbanCol {
                key: SharedString::from(*key),
                label: SharedString::from(*label),
                subtitle: SharedString::from(*subtitle),
                count,
                total: SharedString::from(format!("R$ {total:.2}")),
                orders: ModelRc::new(VecModel::from(list)),
            }
        })
        .collect()
}

/// Resumo dos itens em formato "Coca x2; Pizza x1".
fn format_items_summary(items: &[letaf_core::order::model::OrderItem]) -> String {
    items
        .iter()
        .map(|i| format!("{} x{}", i.product_name, format_qty(i.quantity)))
        .collect::<Vec<_>>()
        .join("; ")
}

