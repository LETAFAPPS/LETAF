use std::sync::Arc;

use chrono::{Datelike, Local, NaiveDate};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use uuid::Uuid;

use letaf_core::customer::model::Customer;
use letaf_core::finance::model::{
    FinanceEntry, FinanceKind, FinanceRecurrence, FinanceStatus, PartyType,
};
use letaf_core::finance::service::CreateFinanceParams;

use crate::context::DesktopState;
use crate::{
    MainWindow, PdvCustomerRow,
};

use super::super::helpers::show_toast;
use super::setup::reapply;
use super::state::{CalStateHandle, CustomersHandle, DueCalState, DueCalStateHandle};
use super::helpers::{parse_amount, parse_date_br};

// ── Trocar aba / busca ──────────────────────────────────────────

pub(crate) fn setup_set_tab(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    cal: CalStateHandle,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_finance_set_tab(move |k| {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_finance_active_tab(SharedString::from(k.to_string()));
        }
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let cal = cal.clone();
        handle.spawn(async move {
            reapply(&ui_weak, &state, &cal).await;
        });
    });
}

pub(crate) fn setup_search(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    cal: CalStateHandle,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_finance_search_changed(move |q| {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_finance_search_query(SharedString::from(q.to_string()));
        }
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let cal = cal.clone();
        handle.spawn(async move {
            reapply(&ui_weak, &state, &cal).await;
        });
    });
}

pub(crate) fn setup_status_filter(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    cal: CalStateHandle,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_finance_set_status_filter(move |k| {
        let key = k.to_string();
        let normalized = match key.as_str() {
            "open" | "overdue" | "settled" => key,
            _ => "all".to_string(),
        };
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_finance_status_filter(SharedString::from(normalized));
        }
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let cal = cal.clone();
        handle.spawn(async move {
            reapply(&ui_weak, &state, &cal).await;
        });
    });
}

/// Troca a página de um grupo de status (paginação de 10 por grupo).
/// A página é gravada na prop correspondente do MainWindow (autoridade
/// no backend); o `reapply` refatia a lista.
pub(crate) fn setup_group_page(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    cal: CalStateHandle,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_finance_set_group_page(move |status, page| {
        let page = page.max(1);
        if let Some(ui) = ui_weak.upgrade() {
            match status.as_str() {
                "overdue" => ui.set_finance_page_overdue(page),
                "pending" => ui.set_finance_page_pending(page),
                "paid" => ui.set_finance_page_settled(page),
                _ => ui.set_finance_page_cancelled(page),
            }
        }
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let cal = cal.clone();
        handle.spawn(async move {
            reapply(&ui_weak, &state, &cal).await;
        });
    });
}

// ── Modal: abrir / fechar ───────────────────────────────────────

pub(crate) fn setup_open_new(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    due_cal: DueCalStateHandle,
    customers: CustomersHandle,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_finance_open_new(move |kind| {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_finance_form_kind(SharedString::from(kind.to_string()));
            ui.set_finance_form_editing_id(SharedString::from(""));
            reset_form(&ui);
            // Reseta o calendário de vencimento para o mês corrente.
            if let Ok(mut g) = due_cal.lock() {
                *g = DueCalState::today();
            }
            ui.set_finance_show_modal(true);
        }
        load_customers_for_modal(&ui_weak, &state, &handle, &customers);
    });
}

pub(crate) fn setup_open_edit(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    due_cal: DueCalStateHandle,
    customers: CustomersHandle,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_finance_open_edit(move |id| {
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let due_cal = due_cal.clone();
        let customers = customers.clone();
        let handle_inner = handle.clone();
        let id = id.to_string();
        handle.spawn(async move {
            let Ok(uuid) = Uuid::parse_str(&id) else { return };
            let cid = state.company_id();
            let entry = match state.finance_service.find_by_id(cid, uuid).await {
                Ok(Some(e)) => e,
                _ => return,
            };
            // Posiciona o calendário no mês do vencimento atual.
            if let Ok(mut g) = due_cal.lock() {
                g.year = entry.due_date.year();
                g.month = entry.due_date.month();
                g.selected = Some(entry.due_date);
            }
            let ui_weak2 = ui_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak2.upgrade() {
                    populate_form(&ui, &entry);
                    ui.set_finance_show_modal(true);
                }
            });
            load_customers_for_modal(&ui_weak, &state, &handle_inner, &customers);
        });
    });
}

/// Carrega a base de clientes (uma vez por abertura do modal) e popula
/// `finance-party-options`. Reusa o cache para filtragens subsequentes.
pub(crate) fn load_customers_for_modal(
    ui_weak: &slint::Weak<MainWindow>,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    customers: &CustomersHandle,
) {
    let ui_weak = ui_weak.clone();
    let state = state.clone();
    let customers = customers.clone();
    handle.spawn(async move {
        let cid = state.company_id();
        let list = state.customer_service.find_all(cid).await.unwrap_or_default();
        if let Ok(mut g) = customers.lock() {
            *g = list.clone();
        }
        let rows = build_party_rows(&list, "");
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_finance_party_options(ModelRc::new(VecModel::from(rows)));
                ui.set_finance_party_search(SharedString::from(""));
            }
        });
    });
}

pub(crate) fn build_party_rows(all: &[Customer], filter: &str) -> Vec<PdvCustomerRow> {
    let needle = filter.trim().to_lowercase();
    let filter_ok = |c: &&Customer| -> bool {
        if needle.is_empty() {
            return true;
        }
        c.name.to_lowercase().contains(&needle)
            || c.phone.as_deref().unwrap_or("").to_lowercase().contains(&needle)
            || c.document.as_deref().unwrap_or("").to_lowercase().contains(&needle)
    };
    let mut rows: Vec<PdvCustomerRow> = all
        .iter()
        .filter(|c| c.base.deleted_at.is_none())
        .filter(filter_ok)
        .map(|c| PdvCustomerRow {
            id: SharedString::from(c.base.id.to_string()),
            name: SharedString::from(c.name.clone()),
            phone: SharedString::from(c.phone.clone().unwrap_or_default()),
            document: SharedString::from(c.document.clone().unwrap_or_default()),
        })
        .collect();
    // Top 50 já filtrados — picker fica responsivo mesmo com base grande.
    rows.truncate(50);
    rows
}

pub(crate) fn setup_close_modal(ui: &MainWindow) {
    let ui_weak = ui.as_weak();
    ui.on_finance_close_modal(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_finance_show_modal(false);
            clear_errors(&ui);
        }
    });
}

pub(crate) fn reset_form(ui: &MainWindow) {
    let today = Local::now().date_naive().format("%d/%m/%Y").to_string();
    ui.set_finance_form_description(SharedString::from(""));
    ui.set_finance_form_party(SharedString::from(""));
    ui.set_finance_form_party_id(SharedString::from(""));
    ui.set_finance_form_category_id(SharedString::from(""));
    ui.set_finance_form_amount(SharedString::from(""));
    ui.set_finance_form_due_date(SharedString::from(today));
    ui.set_finance_form_installments(1);
    ui.set_finance_form_recurrence(SharedString::from("once"));
    ui.set_finance_form_notes(SharedString::from(""));
    ui.set_finance_due_cal_open(false);
    ui.set_finance_show_party_picker(false);
    clear_errors(ui);
}

pub(crate) fn populate_form(ui: &MainWindow, e: &FinanceEntry) {
    ui.set_finance_form_editing_id(SharedString::from(e.base.id.to_string()));
    ui.set_finance_form_kind(SharedString::from(e.kind.to_string()));
    ui.set_finance_form_description(SharedString::from(e.description.clone()));
    ui.set_finance_form_party(SharedString::from(e.party_name.clone()));
    ui.set_finance_form_party_id(SharedString::from(
        e.party_id.map(|i| i.to_string()).unwrap_or_default(),
    ));
    ui.set_finance_form_category_id(SharedString::from(
        e.category_id.map(|i| i.to_string()).unwrap_or_default(),
    ));
    ui.set_finance_form_amount(SharedString::from(format!("{:.2}", e.amount).replace('.', ",")));
    ui.set_finance_form_due_date(SharedString::from(e.due_date.format("%d/%m/%Y").to_string()));
    ui.set_finance_form_installments(e.installment_total);
    ui.set_finance_form_recurrence(SharedString::from(e.recurrence.to_string()));
    ui.set_finance_form_notes(SharedString::from(e.notes.clone().unwrap_or_default()));
    ui.set_finance_due_cal_open(false);
    ui.set_finance_show_party_picker(false);
    clear_errors(ui);
}

pub(crate) fn clear_errors(ui: &MainWindow) {
    ui.set_finance_form_error_description(SharedString::from(""));
    ui.set_finance_form_error_amount(SharedString::from(""));
    ui.set_finance_form_error_due_date(SharedString::from(""));
    ui.set_finance_form_error_general(SharedString::from(""));
}

// ── Salvar ──────────────────────────────────────────────────────

pub(crate) fn setup_save_modal(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<tokio::sync::Notify>,
    cal: CalStateHandle,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_finance_save_modal(move || {
        let Some(ui) = ui_weak.upgrade() else { return };

        let kind_s = ui.get_finance_form_kind().to_string();
        let kind = match kind_s.as_str() {
            "receivable" => FinanceKind::Receivable,
            _ => FinanceKind::Payable,
        };
        let description = ui.get_finance_form_description().to_string();
        let party = ui.get_finance_form_party().to_string();
        let party_id_s = ui.get_finance_form_party_id().to_string();
        let amount_s = ui.get_finance_form_amount().to_string();
        let due_date_s = ui.get_finance_form_due_date().to_string();
        let installments = ui.get_finance_form_installments();
        let recurrence_s = ui.get_finance_form_recurrence().to_string();
        let notes = ui.get_finance_form_notes().to_string();
        let editing_id = ui.get_finance_form_editing_id().to_string();

        // Validação UI antes de chamar service (mensagens por campo).
        clear_errors(&ui);
        let mut has_err = false;
        if description.trim().is_empty() {
            ui.set_finance_form_error_description(SharedString::from(
                "Descrição é obrigatória",
            ));
            has_err = true;
        }
        let amount = match parse_amount(&amount_s) {
            Some(a) if a > 0.0 => a,
            _ => {
                ui.set_finance_form_error_amount(SharedString::from(
                    "Informe um valor maior que zero",
                ));
                has_err = true;
                0.0
            }
        };
        let due_date = match parse_date_br(&due_date_s) {
            Some(d) => d,
            None => {
                ui.set_finance_form_error_due_date(SharedString::from(
                    "Use o formato dd/mm/aaaa",
                ));
                has_err = true;
                Local::now().date_naive()
            }
        };
        if has_err {
            return;
        }

        let recurrence = FinanceRecurrence::from_str(&recurrence_s);
        // Categoria foi removida da UI (Fase 13) — sempre None.
        let category_id: Option<Uuid> = None;
        let party_id: Option<Uuid> = Uuid::parse_str(&party_id_s).ok();
        let party_type = if party_id.is_some() {
            match kind {
                FinanceKind::Payable => PartyType::Supplier,
                FinanceKind::Receivable => PartyType::Customer,
            }
        } else if !party.trim().is_empty() {
            // Texto livre digitado em algum momento sem ID associado.
            match kind {
                FinanceKind::Payable => PartyType::Supplier,
                FinanceKind::Receivable => PartyType::Customer,
            }
        } else {
            PartyType::Other
        };

        let cid = state.company_id();
        let ui_weak = ui.as_weak();
        let state = state.clone();
        let sync_notify = sync_notify.clone();
        let cal = cal.clone();
        let editing = !editing_id.is_empty();

        handle.spawn(async move {
            let result = if editing {
                update_existing(
                    &state,
                    cid,
                    &editing_id,
                    description,
                    party,
                    party_id,
                    party_type,
                    category_id,
                    amount,
                    due_date,
                    notes,
                )
                .await
            } else {
                state
                    .finance_service
                    .create(CreateFinanceParams {
                        company_id: cid,
                        kind,
                        description,
                        party_id,
                        party_name: party,
                        party_type,
                        category_id,
                        amount,
                        due_date,
                        payment_method: None,
                        notes: if notes.is_empty() { None } else { Some(notes) },
                        recurrence,
                        installments,
                        order_id: None,
                    })
                    .await
                    .map(|_| ())
            };

            match result {
                Ok(()) => {
                    sync_notify.notify_one();
                    let ui_weak2 = ui_weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak2.upgrade() {
                            ui.set_finance_show_modal(false);
                            show_toast(&ui, "Lançamento Salvo", "success");
                        }
                    });
                    reapply(&ui_weak, &state, &cal).await;
                }
                Err(e) => {
                    let msg = e.to_string();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak.upgrade() {
                            ui.set_finance_form_error_general(SharedString::from(msg));
                        }
                    });
                }
            }
        });
    });
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn update_existing(
    state: &DesktopState,
    cid: Uuid,
    editing_id: &str,
    description: String,
    party: String,
    party_id: Option<Uuid>,
    party_type: PartyType,
    category_id: Option<Uuid>,
    amount: f64,
    due_date: NaiveDate,
    notes: String,
) -> Result<(), letaf_core::error::CoreError> {
    let uuid = Uuid::parse_str(editing_id).map_err(|e| {
        letaf_core::error::CoreError::Validation(format!("ID inválido: {e}"))
    })?;
    let mut entry = state
        .finance_service
        .find_by_id(cid, uuid)
        .await?
        .ok_or_else(|| letaf_core::error::CoreError::NotFound("Lançamento não encontrado".into()))?;

    if entry.status.is_settled() || entry.status == FinanceStatus::Cancelled {
        return Err(letaf_core::error::CoreError::Validation(
            "Lançamento já finalizado não pode ser editado".into(),
        ));
    }
    entry.description = description.trim().to_string();
    entry.party_id = party_id;
    entry.party_name = party.trim().to_string();
    entry.party_type = party_type;
    entry.category_id = category_id;
    entry.amount = amount;
    entry.due_date = due_date;
    entry.notes = if notes.is_empty() { None } else { Some(notes) };
    entry.base.updated_at = chrono::Utc::now().naive_utc();
    entry.base.synced = false;
    state.finance_service.sync_upsert(cid, entry).await
}

// ── Ações de linha ──────────────────────────────────────────────

pub(crate) fn setup_mark_settled(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<tokio::sync::Notify>,
    cal: CalStateHandle,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_finance_mark_settled(move |id| {
        let id = id.to_string();
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let sync_notify = sync_notify.clone();
        let cal = cal.clone();
        handle.spawn(async move {
            let Ok(uuid) = Uuid::parse_str(&id) else { return };
            let cid = state.company_id();
            match state.finance_service.mark_settled(cid, uuid, None).await {
                Ok(_) => {
                    sync_notify.notify_one();
                    let ui_weak2 = ui_weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak2.upgrade() {
                            show_toast(&ui, "Lançamento Baixado", "success");
                        }
                    });
                    reapply(&ui_weak, &state, &cal).await;
                }
                Err(e) => {
                    let msg = e.to_string();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak.upgrade() {
                            show_toast(&ui, &msg, "error");
                        }
                    });
                }
            }
        });
    });
}

pub(crate) fn setup_cancel_entry(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<tokio::sync::Notify>,
    cal: CalStateHandle,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_finance_cancel_entry(move |id| {
        let id = id.to_string();
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let sync_notify = sync_notify.clone();
        let cal = cal.clone();
        handle.spawn(async move {
            let Ok(uuid) = Uuid::parse_str(&id) else { return };
            let cid = state.company_id();
            if state.finance_service.cancel(cid, uuid).await.is_ok() {
                sync_notify.notify_one();
                reapply(&ui_weak, &state, &cal).await;
            }
        });
    });
}

pub(crate) fn setup_delete_entry(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<tokio::sync::Notify>,
    cal: CalStateHandle,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_finance_delete_entry(move |id| {
        let id = id.to_string();
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let sync_notify = sync_notify.clone();
        let cal = cal.clone();
        handle.spawn(async move {
            let Ok(uuid) = Uuid::parse_str(&id) else { return };
            let cid = state.company_id();
            if state.finance_service.delete(cid, uuid).await.is_ok() {
                sync_notify.notify_one();
                reapply(&ui_weak, &state, &cal).await;
            }
        });
    });
}

