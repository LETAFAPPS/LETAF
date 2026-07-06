use std::sync::{Arc, Mutex};
use rust_decimal::prelude::ToPrimitive;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use uuid::Uuid;


use crate::context::DesktopState;
use crate::{MainWindow, PdvAddressRow, PdvCustomerRow};

use super::state::PdvState;
use super::cart::{slint_row_count, slint_row_data};

// ── Customer picker ───────────────────────────────────────────

pub(crate) fn setup_customer_picker(ui: &MainWindow, pdv: Arc<Mutex<PdvState>>) {
    let ui_weak = ui.as_weak();
    ui.on_pdv_open_customer_picker(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        ui_ref.set_pdv_customer_search(SharedString::default());
        populate_customer_rows(&ui_ref, &pdv, "");
        ui_ref.set_pdv_show_customer_picker(true);
    });
}

pub(crate) fn setup_customer_search(ui: &MainWindow, pdv: Arc<Mutex<PdvState>>) {
    let ui_weak = ui.as_weak();
    ui.on_pdv_customer_search_changed(move |q| {
        if let Some(ui_ref) = ui_weak.upgrade() {
            populate_customer_rows(&ui_ref, &pdv, q.as_str());
        }
    });
}

pub(crate) fn setup_pick_customer(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    pdv: Arc<Mutex<PdvState>>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_pdv_pick_customer(move |id| {
        let Ok(uuid) = Uuid::parse_str(id.as_str()) else { return };
        let name = pdv.lock().ok().and_then(|g| {
            g.customers_all.iter().find(|c| c.0 == uuid).map(|c| c.1.clone())
        });
        if let Some(ui_ref) = ui_weak.upgrade() {
            ui_ref.set_pdv_customer_id(SharedString::from(uuid.to_string()));
            if let Some(n) = name {
                ui_ref.set_pdv_customer_name(SharedString::from(n));
            }
            ui_ref.set_pdv_show_customer_picker(false);
        }
        // Carrega endereços do cliente em background. Quando o
        // operador escolher "Entrega", a lista já está populada
        // para auto-fill.
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let pdv = pdv.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            let addresses = state.customer_address_service
                .list(cid, uuid).await
                .unwrap_or_default();
            if let Ok(mut g) = pdv.lock() {
                g.current_customer_addresses = addresses.clone();
            }
            // Fase 3: também busca a carteira do cliente. Se
            // tiver conta aberta, popula campos pdv-wallet-* — o
            // chip "Carteira" aparece automaticamente no Slint.
            let wallet_info = state.wallet_service
                .find_account_by_customer(cid, uuid)
                .await
                .ok()
                .flatten();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    let rows: Vec<PdvAddressRow> = addresses.iter().map(|a| {
                        let summary = format!("{}, {} — {}", a.street, a.number, a.neighborhood);
                        PdvAddressRow {
                            id: SharedString::from(a.base.id.to_string()),
                            label: SharedString::from(a.label.clone()),
                            summary: SharedString::from(summary),
                            street: SharedString::from(a.street.clone()),
                            number: SharedString::from(a.number.clone()),
                            neighborhood: SharedString::from(a.neighborhood.clone()),
                        }
                    }).collect();
                    ui.set_pdv_customer_addresses(ModelRc::new(VecModel::from(rows)));
                    apply_wallet_to_ui(&ui, wallet_info.as_ref());
                }
            });
        });
    });
}

/// Helpers de formatação da carteira do cliente no PDV.
/// Mantidos próximos ao callsite pra evitar arquivo extra.
pub(crate) fn apply_wallet_to_ui(
    ui: &MainWindow,
    account: Option<&letaf_core::wallet::model::WalletAccount>,
) {
    match account {
        Some(a) => {
            let balance = a.balance;
            let available = a.balance + a.credit_limit;
            let tone = if balance < rust_decimal::Decimal::new(-5, 3) { "neg" }
                else if balance > rust_decimal::Decimal::new(5, 3) { "pos" }
                else { "neutral" };
            ui.set_pdv_wallet_has_account(true);
            ui.set_pdv_wallet_account_id(SharedString::from(a.base.id.to_string()));
            ui.set_pdv_wallet_balance_display(SharedString::from(wallet_money_signed(balance)));
            ui.set_pdv_wallet_balance_tone(SharedString::from(tone));
            ui.set_pdv_wallet_credit_limit_display(
                SharedString::from(crate::format::money_br(a.credit_limit)),
            );
            ui.set_pdv_wallet_available_display(SharedString::from(format!(
                "Disponível: {}",
                crate::format::money_br(available),
            )));
            ui.set_pdv_wallet_available_amount(available.to_f64().unwrap_or(0.0) as f32);
        }
        None => {
            ui.set_pdv_wallet_has_account(false);
            ui.set_pdv_wallet_account_id(SharedString::default());
            ui.set_pdv_wallet_balance_display(SharedString::from(crate::format::money_br(rust_decimal::Decimal::ZERO)));
            ui.set_pdv_wallet_balance_tone(SharedString::from("neutral"));
            ui.set_pdv_wallet_credit_limit_display(SharedString::from(crate::format::money_br(rust_decimal::Decimal::ZERO)));
            ui.set_pdv_wallet_available_display(SharedString::default());
            ui.set_pdv_wallet_available_amount(0.0);
            // Se a forma escolhida era "wallet" e o cliente saiu,
            // limpa o método (sem seleção padrão).
            if ui.get_pdv_payment_method().as_str() == "wallet" {
                ui.set_pdv_payment_method(SharedString::default());
            }
        }
    }
}

pub(crate) fn wallet_money_signed(v: rust_decimal::Decimal) -> String {
    if v >= rust_decimal::Decimal::ZERO {
        crate::format::money_br(v)
    } else {
        format!("R$ -{}", crate::format::money_br(-v).trim_start_matches("R$ "))
    }
}

pub(crate) fn setup_clear_customer(ui: &MainWindow, pdv: Arc<Mutex<PdvState>>) {
    let ui_weak = ui.as_weak();
    ui.on_pdv_clear_customer(move || {
        if let Ok(mut g) = pdv.lock() {
            g.current_customer_addresses.clear();
        }
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_pdv_customer_id(SharedString::default());
            ui.set_pdv_customer_name(SharedString::default());
            ui.set_pdv_customer_addresses(
                ModelRc::new(VecModel::from(Vec::<PdvAddressRow>::new())),
            );
            apply_wallet_to_ui(&ui, None);
        }
    });
}

/// `pdv-use-address` — operador clica numa linha do bloco
/// "Endereços do cliente". Popula os campos editáveis do carrinho
/// com street/number/neighborhood do endereço escolhido.
pub(crate) fn setup_use_address(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_pdv_use_address(move |id| {
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        let rows = ui_ref.get_pdv_customer_addresses();
        for i in 0..slint_row_count(&rows) {
            let Some(row) = slint_row_data(&rows, i) else { continue };
            if row.id.as_str() == id.as_str() {
                ui_ref.set_pdv_delivery_street(row.street);
                ui_ref.set_pdv_delivery_number(row.number);
                ui_ref.set_pdv_delivery_neighborhood(row.neighborhood);
                return;
            }
        }
    });
}

pub(crate) fn populate_customer_rows(ui: &MainWindow, pdv: &Arc<Mutex<PdvState>>, query: &str) {
    let q_lower = query.to_lowercase();
    let rows: Vec<PdvCustomerRow> = pdv.lock().ok().map(|g| {
        g.customers_all.iter()
            .filter(|(_, name, phone, doc)| {
                if q_lower.is_empty() { return true; }
                name.to_lowercase().contains(&q_lower)
                    || phone.as_deref().map(|p| p.contains(&q_lower)).unwrap_or(false)
                    || doc.as_deref().map(|d| d.contains(&q_lower)).unwrap_or(false)
            })
            .take(80)  // limita render para não estourar com muitos clientes
            .map(|(id, name, phone, doc)| PdvCustomerRow {
                id: SharedString::from(id.to_string()),
                name: SharedString::from(name.clone()),
                phone: SharedString::from(phone.clone().unwrap_or_default()),
                document: SharedString::from(doc.clone().unwrap_or_default()),
            })
            .collect()
    }).unwrap_or_default();
    ui.set_pdv_customer_rows(ModelRc::new(VecModel::from(rows)));
}

