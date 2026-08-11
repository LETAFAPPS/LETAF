use std::collections::HashMap;
use rust_decimal::prelude::ToPrimitive;
use std::sync::Arc;

use chrono::Utc;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use uuid::Uuid;

use letaf_core::customer::model::Customer;
use letaf_core::customer_address::model::CustomerAddress;
use letaf_core::order::model::{Order, OrderStatus};

use crate::context::DesktopState;
use crate::format::{format_document, format_phone};
use crate::{CustomerAddressRow, CustomerOrderRow, MainWindow};

use super::data::{AddressRow, DecodedCustomer, money, order_summary, recency_label, status_for, status_label_pt};
use super::crud::{decode_customer_pixel_buffer, decoded_to_customer_data_ref};
use crate::CustomersState;

/// Callback: carrega clientes + agrega métricas dos pedidos.
pub(crate) fn setup_refresh_customers(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    cache: Arc<std::sync::Mutex<Vec<DecodedCustomer>>>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.global::<CustomersState>().on_refresh_customers(move || {
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let cache = cache.clone();
        // Clone extra do handle para recarregar o detalhe da seleção sob demanda.
        let handle_detail = handle.clone();

        handle.spawn(async move {
            let cid = state.company_id();
            let customers = match state.customer_service.find_all(cid).await {
                Ok(c) => c,
                Err(e) => { tracing::error!("Failed to load customers: {e}"); return; }
            };
            // §13: carrega os pedidos SEM itens (`find_all_light`) — a lista só
            // precisa de total/data/status para LTV, ticket, VIP e recência. O
            // histórico com resumo de itens é carregado SOB DEMANDA ao selecionar
            // o cliente (`load_customer_detail`), evitando hidratar os itens de
            // TODOS os pedidos de TODOS os clientes a cada refresh.
            let orders = state.order_service.find_all_light(cid).await.unwrap_or_default();
            // Endereços de TODOS os clientes numa única query (evita N+1)
            // e agrupa por customer_id em memória.
            let mut addrs: HashMap<Uuid, Vec<CustomerAddress>> = HashMap::new();
            if let Ok(all) = state.customer_address_service.list_by_company(cid).await {
                for a in all {
                    addrs.entry(a.customer_id).or_default().push(a);
                }
            }

            let decoded = tokio::task::spawn_blocking(move || {
                build_decoded(&customers, &orders, &addrs)
            }).await.unwrap_or_default();

            if let Ok(mut g) = cache.lock() { *g = decoded; }

            let cache2 = cache.clone();
            let state2 = state.clone();
            let ui_weak2 = ui_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak2.upgrade() else { return };
                let data = cache2.lock().map(|g| {
                    g.iter().map(decoded_to_customer_data_ref).collect::<Vec<_>>()
                }).unwrap_or_default();
                ui.global::<CustomersState>().set_customers(ModelRc::new(VecModel::from(data)));
                // Recarrega o detalhe (com itens) da seleção atual SOB DEMANDA,
                // para refletir criação/edição sem o operador trocar de tela.
                let sel = ui.global::<CustomersState>().get_selected_customer_id().to_string();
                if !sel.is_empty() {
                    handle_detail.spawn(load_customer_detail(
                        ui_weak2.clone(), state2.clone(), cache2.clone(), sel,
                    ));
                }
            });
        });
    });
}

/// Agrega pedidos por cliente e calcula LTV / ticket / status / VIP.
pub(crate) fn build_decoded(
    customers: &[Customer],
    orders: &[Order],
    addrs: &HashMap<Uuid, Vec<CustomerAddress>>,
) -> Vec<DecodedCustomer> {
    let now = Utc::now().naive_utc();

    // Agrupa pedidos por customer_id.
    let mut by_cust: HashMap<Uuid, Vec<&Order>> = HashMap::new();
    for o in orders {
        by_cust.entry(o.customer_id).or_default().push(o);
    }

    // LTV por cliente (pedidos não-cancelados) para o percentil VIP.
    let ltvs: Vec<(Uuid, f64)> = customers.iter().map(|c| {
        let ltv = by_cust.get(&c.base.id).map(|v| v.iter()
            .filter(|o| o.status != OrderStatus::Cancelled)
            .map(|o| o.total.to_f64().unwrap_or(0.0)).sum::<f64>()).unwrap_or(0.0);
        (c.base.id, ltv)
    }).collect();
    let total_customers = customers.len().max(1);

    customers.iter().map(|c| {
        let mut list: Vec<&Order> = by_cust.get(&c.base.id).cloned().unwrap_or_default();
        list.sort_by_key(|o| std::cmp::Reverse(o.base.created_at));
        let active: Vec<&&Order> = list.iter()
            .filter(|o| o.status != OrderStatus::Cancelled).collect();

        let ltv: f64 = active.iter().map(|o| o.total.to_f64().unwrap_or(0.0)).sum();
        let count = active.len() as i32;
        let avg = if count > 0 { ltv / count as f64 } else { 0.0 };

        let last = list.first().map(|o| o.base.created_at);
        let days = last.map(|d| (now - d).num_days());
        let (status, status_label) = status_for(days);
        let last_order = last
            .map(|d| letaf_core::tz::to_local(d).format("%d/%m").to_string())
            .unwrap_or_default();

        // Posição no ranking de LTV (1 = maior LTV entre todos).
        let my_ltv = ltvs.iter().find(|(id, _)| *id == c.base.id)
            .map(|(_, v)| *v).unwrap_or(0.0);
        let rank = ltvs.iter().filter(|(_, v)| *v > my_ltv).count() + 1;
        let is_vip = my_ltv > 0.0 && rank * 5 <= total_customers;
        let ltv_pct = if count > 0 {
            format!("Top {rank}º")
        } else {
            "Sem Pedidos".to_string()
        };

        // O histórico (`recent`, com resumo de itens) NÃO é montado aqui: seria
        // preciso hidratar os itens de todos os pedidos. É carregado sob demanda
        // ao selecionar o cliente (`load_customer_detail`). §13.
        let addresses: Vec<AddressRow> = addrs.get(&c.base.id)
            .map(|v| v.iter().map(|a| {
                let apt = a.apartment.as_deref()
                    .filter(|s| !s.is_empty())
                    .map(|s| format!(" - Ap. {s}"))
                    .unwrap_or_default();
                AddressRow {
                    id: SharedString::from(a.base.id.to_string()),
                    label: SharedString::from(a.display_label()),
                    line: SharedString::from(format!(
                        "{}, {}, {}{}", a.street, a.number, a.neighborhood, apt
                    )),
                }
            }).collect())
            .unwrap_or_default();

        DecodedCustomer {
            id: SharedString::from(c.base.id.to_string()),
            name: SharedString::from(c.name.as_str()),
            email: SharedString::from(c.email.as_deref().unwrap_or("")),
            phone: SharedString::from(c.phone.as_deref().map(format_phone).unwrap_or_default()),
            document: SharedString::from(c.document.as_deref().map(format_document).unwrap_or_default()),
            avatar_initial: SharedString::from(
                c.name.chars().next().map(|ch| ch.to_uppercase().to_string())
                    .unwrap_or_else(|| "?".to_string()),
            ),
            notes: SharedString::from(c.notes.as_deref().unwrap_or("")),
            created_at: SharedString::from(
                letaf_core::tz::to_local(c.base.created_at).format("%d/%m/%Y").to_string(),
            ),
            ltv: SharedString::from(money(ltv)),
            ltv_pct: SharedString::from(ltv_pct),
            order_count: count,
            avg_ticket: SharedString::from(money(avg)),
            last_order: SharedString::from(last_order),
            last_order_rel: SharedString::from(recency_label(days)),
            status: SharedString::from(status),
            status_label: SharedString::from(status_label),
            is_vip,

            addresses,
            pixel_buffer: c.profile_picture.as_deref()
                .filter(|s| !s.is_empty())
                .and_then(decode_customer_pixel_buffer),
        }
    }).collect()
}

/// Callback: filtra clientes pelo texto de pesquisa (event loop).
/// Busca textual da lista de clientes. O filtro reconstrói o VecModel de
/// clientes; com debounce ele só re-renderiza quando o usuário para de
/// digitar, evitando refazer a lista a cada tecla (§13).
pub(crate) fn setup_filter_customers(
    ui: &MainWindow,
    cache: Arc<std::sync::Mutex<Vec<DecodedCustomer>>>,
) {
    let ui_weak = ui.as_weak();
    let timer = std::rc::Rc::new(slint::Timer::default());
    ui.global::<CustomersState>().on_filter_customers(move |query| {
        let q = query.to_lowercase();
        let ui_weak = ui_weak.clone();
        let cache = cache.clone();
        super::super::helpers::debounce(&timer, move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            let data = cache.lock().map(|g| {
                g.iter()
                    .filter(|c| {
                        if q.is_empty() { return true; }
                        c.name.to_lowercase().contains(q.as_str())
                            || c.email.to_lowercase().contains(q.as_str())
                            || c.phone.to_lowercase().contains(q.as_str())
                            || c.document.to_lowercase().contains(q.as_str())
                    })
                    .map(decoded_to_customer_data_ref)
                    .collect::<Vec<_>>()
            }).unwrap_or_default();
            ui.global::<CustomersState>().set_customers(ModelRc::new(VecModel::from(data)));
        });
    });
}

/// Carrega o histórico (COM itens) do cliente selecionado SOB DEMANDA e popula
/// o painel de detalhe. §13: a lista usa `find_all_light` (sem itens); só os
/// pedidos DAQUELE cliente são hidratados aqui (conjunto pequeno). Cliente +
/// endereços vêm do cache agregado já em memória.
pub(crate) async fn load_customer_detail(
    ui_weak: slint::Weak<MainWindow>,
    state: DesktopState,
    cache: Arc<std::sync::Mutex<Vec<DecodedCustomer>>>,
    id: String,
) {
    let Ok(customer_id) = Uuid::parse_str(&id) else { return };
    let cid = state.company_id();
    let mut orders = state
        .order_service
        .find_by_customer(cid, customer_id)
        .await
        .unwrap_or_default();
    orders.sort_by_key(|o| std::cmp::Reverse(o.base.created_at));
    // Formata as linhas (com resumo dos itens) FORA do event loop — Strings são
    // Send; os `CustomerOrderRow` (SharedString) são montados no event loop.
    let rows_raw: Vec<[String; 7]> = orders
        .iter()
        .map(|o| {
            [
                o.base.id.to_string(),
                format!("#{:04}", o.number),
                order_summary(o),
                letaf_core::tz::to_local(o.base.created_at).format("%d/%m").to_string(),
                o.status.to_string(),
                status_label_pt(&o.status).to_string(),
                money(o.total.to_f64().unwrap_or(0.0)),
            ]
        })
        .collect();
    let _ = slint::invoke_from_event_loop(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        // Guarda de seleção: se o operador já trocou de cliente enquanto esta
        // carga estava em voo, descarta — senão a query mais lenta sobrescreveria
        // o detalhe do cliente atualmente selecionado (detalhe stale).
        if ui.global::<CustomersState>().get_selected_customer_id().as_str() != id {
            return;
        }
        // Cliente + endereços do cache agregado (já em memória).
        let Some((data, addrs)) = cache.lock().ok().and_then(|g| {
            g.iter().find(|c| c.id == id).map(|d| {
                let addrs: Vec<CustomerAddressRow> = d
                    .addresses
                    .iter()
                    .map(|a| CustomerAddressRow {
                        id: a.id.clone(),
                        label: a.label.clone(),
                        line: a.line.clone(),
                    })
                    .collect();
                (decoded_to_customer_data_ref(d), addrs)
            })
        }) else {
            return;
        };
        let rows: Vec<CustomerOrderRow> = rows_raw
            .into_iter()
            .map(|[oid, number, summary, date, status, status_label, total]| CustomerOrderRow {
                id: oid.into(),
                number: number.into(),
                summary: summary.into(),
                date: date.into(),
                status: status.into(),
                status_label: status_label.into(),
                total: total.into(),
            })
            .collect();
        // `selected_customer_id` já foi setado sincronamente no handler de
        // clique (e confirmado pela guarda acima) — não re-seta aqui.
        let st = ui.global::<CustomersState>();
        st.set_detail_customer(data);
        st.set_detail_recent_orders(ModelRc::new(VecModel::from(rows)));
        st.set_detail_addresses(ModelRc::new(VecModel::from(addrs)));
    });
}

pub(crate) fn setup_select_customer(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    cache: Arc<std::sync::Mutex<Vec<DecodedCustomer>>>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.global::<CustomersState>().on_select_customer(move |id| {
        // Marca a seleção SINCRONAMENTE (na thread da UI): o destaque muda na
        // hora e a guarda em `load_customer_detail` sabe qual é a seleção
        // vigente — a carga assíncrona só aplica o detalhe se ainda for esta.
        if let Some(ui) = ui_weak.upgrade() {
            ui.global::<CustomersState>().set_selected_customer_id(id.clone());
        }
        handle.spawn(load_customer_detail(
            ui_weak.clone(),
            state.clone(),
            cache.clone(),
            id.to_string(),
        ));
    });
}

