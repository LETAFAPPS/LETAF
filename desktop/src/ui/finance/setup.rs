use std::sync::Arc;

use slint::{ComponentHandle, SharedString};
use uuid::Uuid;

use letaf_core::finance::model::FinanceKind;

use crate::context::DesktopState;
use crate::format::money_br;
use crate::MainWindow;

use super::state::{CalState, CalStateHandle, CustomersHandle, DueCalState, DueCalStateHandle};
use super::snapshot::{apply_snapshot, build_snapshot};
use super::modal::{setup_cancel_entry, setup_close_modal, setup_delete_entry, setup_group_page, setup_mark_settled, setup_open_edit, setup_open_new, setup_save_modal, setup_search, setup_set_tab, setup_status_filter};
use super::calendar::{setup_cal_next, setup_cal_prev, setup_cal_select_day, setup_cal_today, setup_due_cal, setup_party_picker, setup_set_category, setup_set_installments, setup_set_recurrence, setup_set_view};

pub(crate) fn setup_finance(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<tokio::sync::Notify>,
    sync_cycle_done: Arc<tokio::sync::Notify>,
) {
    let cal_state: CalStateHandle = Arc::new(std::sync::Mutex::new(CalState::today()));
    let due_cal: DueCalStateHandle = Arc::new(std::sync::Mutex::new(DueCalState::today()));
    let customers: CustomersHandle = Arc::new(std::sync::Mutex::new(Vec::new()));

    setup_refresh(ui, state, handle, cal_state.clone());
    setup_set_tab(ui, state, handle, cal_state.clone());
    setup_search(ui, state, handle, cal_state.clone());
    setup_status_filter(ui, state, handle, cal_state.clone());
    setup_group_page(ui, state, handle, cal_state.clone());
    setup_open_new(ui, state, handle, due_cal.clone(), customers.clone());
    setup_open_edit(ui, state, handle, due_cal.clone(), customers.clone());
    setup_close_modal(ui);
    setup_save_modal(ui, state, handle, sync_notify.clone(), cal_state.clone());
    setup_mark_settled(ui, state, handle, sync_notify.clone(), cal_state.clone());
    setup_cancel_entry(ui, state, handle, sync_notify.clone(), cal_state.clone());
    setup_delete_entry(ui, state, handle, sync_notify, cal_state.clone());
    setup_set_recurrence(ui);
    setup_set_installments(ui);
    setup_set_category(ui);
    setup_set_view(ui, state, handle, cal_state.clone());
    setup_cal_prev(ui, state, handle, cal_state.clone());
    setup_cal_next(ui, state, handle, cal_state.clone());
    setup_cal_today(ui, state, handle, cal_state.clone());
    setup_cal_select_day(ui, state, handle, cal_state.clone());
    setup_party_picker(ui, customers.clone());
    setup_due_cal(ui, due_cal);
    setup_settle_confirm(ui, state, handle);
    setup_delete_confirm(ui, state, handle);
    setup_sync_listener(ui, state, handle, sync_cycle_done, cal_state);
}

/// Modal de confirmação de exclusão. Mesma mecânica do
/// `setup_settle_confirm`: `request` popula campos + abre o modal;
/// `confirm` dispara `finance-delete-entry` (callback já registrado).
pub(crate) fn setup_delete_confirm(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
) {
    let ui_weak = ui.as_weak();
    let state_req = state.clone();
    let handle_req = handle.clone();
    ui.on_finance_request_delete(move |id| {
        let id_s = id.to_string();
        let Ok(uuid) = Uuid::parse_str(&id_s) else { return };
        let ui_weak = ui_weak.clone();
        let state = state_req.clone();
        handle_req.spawn(async move {
            let cid = state.company_id();
            let entry = match state.finance_service.find_by_id(cid, uuid).await {
                Ok(Some(e)) => e,
                _ => return,
            };
            let amount = match entry.kind {
                FinanceKind::Receivable => format!("+{}", money_br(entry.amount)),
                FinanceKind::Payable => format!("−{}", money_br(entry.amount)),
            };
            let kind_s = entry.kind.to_string();
            let desc = entry.description.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    ui.set_finance_delete_target_id(SharedString::from(id_s));
                    ui.set_finance_delete_target_desc(SharedString::from(desc));
                    ui.set_finance_delete_target_amount(SharedString::from(amount));
                    ui.set_finance_delete_target_kind(SharedString::from(kind_s));
                    ui.set_finance_show_delete_confirm(true);
                }
            });
        });
    });

    let ui_weak = ui.as_weak();
    ui.on_finance_close_delete_confirm(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_finance_show_delete_confirm(false);
            ui.set_finance_delete_target_id(SharedString::from(""));
        }
    });

    let ui_weak = ui.as_weak();
    ui.on_finance_confirm_delete(move || {
        if let Some(ui) = ui_weak.upgrade() {
            let id = ui.get_finance_delete_target_id();
            ui.set_finance_show_delete_confirm(false);
            ui.set_finance_delete_target_id(SharedString::from(""));
            if !id.is_empty() {
                ui.invoke_finance_delete_entry(id);
            }
        }
    });
}

/// Abre/fecha o modal de confirmação de baixa (Receber/Pagar).
/// Popula desc/amount/party/kind no MainWindow a partir do entry
/// encontrado em `find_by_id`, para o modal poder mostrar sem
/// duplicar lógica de formatação.
pub(crate) fn setup_settle_confirm(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
) {
    // request: abre o modal com os dados do entry
    let ui_weak = ui.as_weak();
    let state_req = state.clone();
    let handle_req = handle.clone();
    ui.on_finance_request_settle(move |id| {
        let id_s = id.to_string();
        let Ok(uuid) = Uuid::parse_str(&id_s) else { return };
        let ui_weak = ui_weak.clone();
        let state = state_req.clone();
        handle_req.spawn(async move {
            let cid = state.company_id();
            let entry = match state.finance_service.find_by_id(cid, uuid).await {
                Ok(Some(e)) => e,
                _ => return,
            };
            let amount = match entry.kind {
                FinanceKind::Receivable => format!("+{}", money_br(entry.amount)),
                FinanceKind::Payable => format!("−{}", money_br(entry.amount)),
            };
            let kind_s = entry.kind.to_string();
            let desc = entry.description.clone();
            let party = entry.party_name.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    ui.set_finance_settle_target_id(SharedString::from(id_s));
                    ui.set_finance_settle_target_kind(SharedString::from(kind_s));
                    ui.set_finance_settle_target_desc(SharedString::from(desc));
                    ui.set_finance_settle_target_party(SharedString::from(party));
                    ui.set_finance_settle_target_amount(SharedString::from(amount));
                    ui.set_finance_show_settle_confirm(true);
                }
            });
        });
    });

    // close: só fecha
    let ui_weak = ui.as_weak();
    ui.on_finance_close_settle_confirm(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_finance_show_settle_confirm(false);
            ui.set_finance_settle_target_id(SharedString::from(""));
        }
    });

    // confirm: fecha o modal + dispara mark-settled (já registrado)
    let ui_weak = ui.as_weak();
    ui.on_finance_confirm_settle(move || {
        if let Some(ui) = ui_weak.upgrade() {
            let id = ui.get_finance_settle_target_id();
            ui.set_finance_show_settle_confirm(false);
            ui.set_finance_settle_target_id(SharedString::from(""));
            if !id.is_empty() {
                ui.invoke_finance_mark_settled(id);
            }
        }
    });
}

/// Escuta o fim de cada ciclo de sync e re-refresh a aba quando ela
/// está visível. Padrão idêntico ao dashboard/reports.
pub(crate) fn setup_sync_listener(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    cycle_done: Arc<tokio::sync::Notify>,
    cal: CalStateHandle,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    handle.spawn(async move {
        loop {
            cycle_done.notified().await;
            let visible = {
                let ui_weak2 = ui_weak.clone();
                let (tx, rx) = std::sync::mpsc::channel();
                let _ = slint::invoke_from_event_loop(move || {
                    let active = ui_weak2
                        .upgrade()
                        .map(|u| u.get_active_tab().to_string())
                        .unwrap_or_default();
                    let _ = tx.send(active == "finance");
                });
                rx.recv().unwrap_or(false)
            };
            if !visible {
                continue;
            }
            reapply(&ui_weak, &state, &cal).await;
        }
    });
}

// ── Refresh ──────────────────────────────────────────────────────

pub(crate) fn setup_refresh(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    cal: CalStateHandle,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    ui.on_finance_refresh(move || {
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let cal = cal.clone();
        handle.spawn(async move {
            reapply(&ui_weak, &state, &cal).await;
        });
    });
}

/// Busca dados frescos e despacha pra UI. Roda fora do event loop
/// (tokio task) — usa `invoke_from_event_loop` pra setar.
pub(crate) async fn reapply(
    ui_weak: &slint::Weak<MainWindow>,
    state: &DesktopState,
    cal: &CalStateHandle,
) {
    let cid = state.company_id();
    let entries = state.finance_service.find_all(cid).await.unwrap_or_default();
    let categories = state
        .finance_category_service
        .find_all(cid)
        .await
        .unwrap_or_default();
    // Vendas pagas (PDV) entram no fluxo de caixa do dia em que foram
    // criadas — decisão do usuário na Fase 1 (AskUserQuestion).
    let orders = state.order_service.find_all(cid).await.unwrap_or_default();
    let cal_snapshot = cal.lock().ok().map(|g| g.clone()).unwrap_or_else(CalState::today);
    // Badge da sidebar: contas vencidas (mesmo critério do KPI "VENCIDOS").
    let overdue = overdue_count(&entries);
    let snap = build_snapshot(&entries, &categories, &orders, &cal_snapshot);
    let ui_weak = ui_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_finance_overdue_count(overdue);
            apply_snapshot(&ui, snap);
        }
    });
}

/// Conta os lançamentos vencidos (não liquidados nem cancelados).
/// Fonte única do badge da sidebar (reapply + recompute de badges).
pub(crate) fn overdue_count(entries: &[letaf_core::finance::model::FinanceEntry]) -> i32 {
    let today = chrono::Local::now().date_naive();
    entries.iter().filter(|e| e.is_overdue(today)).count() as i32
}

