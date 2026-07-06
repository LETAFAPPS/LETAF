use std::collections::HashMap;
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

use super::data::{AddressRow, DecodedCustomer, money, order_summary, recency_label, RecentOrder, status_for, status_label_pt};
use super::crud::{decode_customer_pixel_buffer, decoded_to_customer_data_ref};

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

    ui.on_refresh_customers(move || {
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let cache = cache.clone();

        handle.spawn(async move {
            let cid = state.company_id();
            let customers = match state.customer_service.find_all(cid).await {
                Ok(c) => c,
                Err(e) => { tracing::error!("Failed to load customers: {e}"); return; }
            };
            // Carrega todos os pedidos uma vez e agrupa por cliente.
            let orders = state.order_service.find_all(cid).await.unwrap_or_default();
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
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                let data = cache2.lock().map(|g| {
                    g.iter().map(decoded_to_customer_data_ref).collect::<Vec<_>>()
                }).unwrap_or_default();
                ui.set_customers(ModelRc::new(VecModel::from(data)));
                // Reaplica a seleção atual para o detalhe refletir
                // criação/edição sem o operador trocar de tela.
                let sel = ui.get_selected_customer_id().to_string();
                if !sel.is_empty() {
                    apply_selection(&ui, &cache2, &sel);
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
            .map(|o| o.total).sum::<f64>()).unwrap_or(0.0);
        (c.base.id, ltv)
    }).collect();
    let total_customers = customers.len().max(1);

    customers.iter().map(|c| {
        let mut list: Vec<&Order> = by_cust.get(&c.base.id).cloned().unwrap_or_default();
        list.sort_by_key(|o| std::cmp::Reverse(o.base.created_at));
        let active: Vec<&&Order> = list.iter()
            .filter(|o| o.status != OrderStatus::Cancelled).collect();

        let ltv: f64 = active.iter().map(|o| o.total).sum();
        let count = active.len() as i32;
        let avg = if count > 0 { ltv / count as f64 } else { 0.0 };

        let last = list.first().map(|o| o.base.created_at);
        let days = last.map(|d| (now - d).num_days());
        let (status, status_label) = status_for(days);
        let last_order = last.map(|d| d.format("%d/%m").to_string())
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

        // Todos os pedidos (a paginação é feita na UI, 5 por página).
        let recent: Vec<RecentOrder> = list.iter().map(|o| RecentOrder {
            id: SharedString::from(o.base.id.to_string()),
            number: SharedString::from(format!("#{:04}", o.number)),
            summary: SharedString::from(order_summary(o)),
            date: SharedString::from(o.base.created_at.format("%d/%m").to_string()),
            status: SharedString::from(o.status.to_string()),
            status_label: SharedString::from(status_label_pt(&o.status)),
            total: SharedString::from(money(o.total)),
        }).collect();

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
            created_at: SharedString::from(c.base.created_at.format("%d/%m/%Y").to_string()),
            ltv: SharedString::from(money(ltv)),
            ltv_pct: SharedString::from(ltv_pct),
            order_count: count,
            avg_ticket: SharedString::from(money(avg)),
            last_order: SharedString::from(last_order),
            last_order_rel: SharedString::from(recency_label(days)),
            status: SharedString::from(status),
            status_label: SharedString::from(status_label),
            is_vip,
            recent,
            addresses,
            pixel_buffer: c.profile_picture.as_deref()
                .filter(|s| !s.is_empty())
                .and_then(decode_customer_pixel_buffer),
        }
    }).collect()
}

/// Callback: filtra clientes pelo texto de pesquisa (event loop).
pub(crate) fn setup_filter_customers(
    ui: &MainWindow,
    cache: Arc<std::sync::Mutex<Vec<DecodedCustomer>>>,
) {
    let ui_weak = ui.as_weak();
    ui.on_filter_customers(move |query| {
        let Some(ui) = ui_weak.upgrade() else { return };
        let q = query.to_lowercase();
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
        ui.set_customers(ModelRc::new(VecModel::from(data)));
    });
}

/// Callback: seleção de um cliente → popula o painel de detalhe.
/// Popula o painel de detalhe (cliente + pedidos + endereços) a partir
/// do cache. Reutilizado pela seleção e pela re-seleção pós-refresh
/// (atualiza a tela sem o operador trocar de aba).
pub(crate) fn apply_selection(ui: &MainWindow, cache: &std::sync::Mutex<Vec<DecodedCustomer>>, id: &str) {
    let found = cache.lock().ok().and_then(|g|
        g.iter().find(|c| c.id == id).map(|d| {
            let data = decoded_to_customer_data_ref(d);
            let rows: Vec<CustomerOrderRow> = d.recent.iter().map(|r| CustomerOrderRow {
                id: r.id.clone(),
                number: r.number.clone(),
                summary: r.summary.clone(),
                date: r.date.clone(),
                status: r.status.clone(),
                status_label: r.status_label.clone(),
                total: r.total.clone(),
            }).collect();
            let addrs: Vec<CustomerAddressRow> = d.addresses.iter().map(|a| CustomerAddressRow {
                id: a.id.clone(),
                label: a.label.clone(),
                line: a.line.clone(),
            }).collect();
            (data, rows, addrs)
        }));
    if let Some((data, rows, addrs)) = found {
        ui.set_selected_customer_id(SharedString::from(id));
        ui.set_detail_customer(data);
        ui.set_detail_recent_orders(ModelRc::new(VecModel::from(rows)));
        ui.set_detail_addresses(ModelRc::new(VecModel::from(addrs)));
    }
}

pub(crate) fn setup_select_customer(
    ui: &MainWindow,
    cache: Arc<std::sync::Mutex<Vec<DecodedCustomer>>>,
) {
    let ui_weak = ui.as_weak();
    ui.on_select_customer(move |id| {
        let Some(ui) = ui_weak.upgrade() else { return };
        apply_selection(&ui, &cache, id.as_str());
    });
}

