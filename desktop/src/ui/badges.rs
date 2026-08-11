//! Recompute em tempo real dos badges da sidebar (Pedidos, Financeiro,
//! Estoque e Assinatura).
//!
//! Um ÚNICO ouvinte reage ao `badges_dirty` que o SyncWorker dispara ao
//! fim de cada ciclo. Como toda escrita local aciona um ciclo de sync
//! (offline-first §7.3), os badges refletem qualquer mudança — local ou
//! vinda do pull — sem o operador trocar de aba.
//!
//! Usa um Notify DEDICADO (não o `cycle_done` compartilhado por 7 telas):
//! com um só ouvinte, `notify_one` bufferiza o permit e nunca perde um
//! ciclo — garantindo o "tempo real" de fato.
use std::sync::Arc;

use slint::ComponentHandle;
use tokio::sync::Notify;

use crate::context::DesktopState;
use crate::{CashState, MainWindow};

/// Lê o SQLite local, recalcula os 4 contadores e pinta na UI num único
/// `invoke_from_event_loop`. Toda derivação no Rust (§3/§11); a UI só
/// exibe o número. Isolado por `company_id`.
pub(crate) async fn refresh_all_badges(ui_weak: &slint::Weak<MainWindow>, state: &DesktopState) {
    let cid = state.company_id();
    let today = letaf_core::tz::today();

    // COUNT(*) direto: este refresh roda a cada ciclo de sync (30 s) e
    // carregar pedidos (com itens), lançamentos e o catálogo inteiro
    // (com o base64 das fotos) só para produzir três inteiros era o
    // trabalho mais caro do aplicativo ocioso. §13.
    let orders_n = state.order_service.count_active(cid).await.unwrap_or(0) as i32;
    let stock_n = state
        .product_service
        .count_out_of_stock(cid)
        .await
        .unwrap_or(0) as i32;
    let entries = state.finance_service.find_all(cid).await.unwrap_or_default();
    let sub_pending = state
        .subscription_service
        .pending_summary(cid, today)
        .await
        .map(|s| s.action_count as i32)
        .unwrap_or(0);

    let overdue_n = super::finance::overdue_count(&entries);

    // Bloqueio da PDV: reflete em TEMPO REAL (independente da aba ativa) a
    // abertura/fechamento de caixa feita em OUTRO terminal — a PDV consome
    // `cash-blocked` e ficava presa no estado do último `cash_refresh`. Só o
    // booleano (via `find_active`); NÃO toca os campos do modal de caixa.
    let cash_blocked = state
        .cash_service
        .find_active(cid)
        .await
        .ok()
        .flatten()
        .is_none();

    let ui_weak = ui_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_orders_active_count(orders_n);
            ui.set_finance_overdue_count(overdue_n);
            ui.set_stock_out_count(stock_n);
            ui.set_subscription_pending_count(sub_pending);
            ui.global::<CashState>().set_cash_blocked(cash_blocked);
        }
    });
}

/// Ouve o `badges_dirty` (um ciclo de sync terminou) e recalcula. Pinta
/// uma vez no startup para os badges já aparecerem sem abrir as abas.
pub(crate) fn setup_badges_listener(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    badges_dirty: Arc<Notify>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    handle.spawn(async move {
        refresh_all_badges(&ui_weak, &state).await;
        loop {
            badges_dirty.notified().await;
            refresh_all_badges(&ui_weak, &state).await;
        }
    });
}
