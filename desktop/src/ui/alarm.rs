//! Wireup do alarme de novos pedidos no `MainWindow` Slint.
//!
//! Responsabilidades (mantidas pequenas e independentes — AI_RULES.md §8):
//! - `setup_alarm`: ponto único de wireup; chama os outros sub-setups.
//! - `seed_watcher_on_boot`: pré-popula o `AlarmWatcher` com os pedidos
//!   pendentes já existentes no banco no instante em que o app abre.
//!   Sem isso, todo pedido pendente disparaia alarme indevido no
//!   primeiro ciclo de sync (mau UX).
//! - `setup_alarm_observer`: task tokio que escuta `alarm_signal` do
//!   SyncWorker e abre o modal no event loop Slint.
//! - `setup_alarm_acknowledge` / `setup_alarm_dismiss`: callbacks do
//!   próprio modal — "Ver pedidos" e fechar (mantém alarme rugindo).
//!
//! Toda a decisão de QUANDO tocar/parar fica em Rust; o Slint só
//! observa propriedades e dispara callbacks (AI_RULES.md §1, §14).

use slint::{ComponentHandle, SharedString};

use crate::MainWindow;
use crate::context::DesktopState;

/// Ponto de entrada chamado pelo `setup_callbacks` em [`super::mod`].
pub fn setup_alarm(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
) {
    seed_watcher_on_boot(state, handle);
    setup_alarm_observer(ui, state, handle);
    setup_alarm_acknowledge(ui, state);
    setup_alarm_dismiss(ui, state);
}

/// Lê os pedidos do banco local e marca como "já vistos" os que estão
/// em `Pending`. Roda uma única vez na inicialização — pedidos
/// puxados depois pelo SyncWorker entram via `AlarmWatcher::note()`.
///
/// Por que não usar `cycle_done` para fazer um "warm-up cycle"?
/// Porque o primeiro ciclo de sync pode trazer realmente pedidos
/// novos (criados enquanto o app estava fechado); queremos alarmar
/// para esses. O seed só toca nos pedidos que JÁ estão no SQLite
/// local — eles foram sincronizados em sessões anteriores e
/// presumivelmente já foram vistos pelo operador.
fn seed_watcher_on_boot(state: &DesktopState, handle: &tokio::runtime::Handle) {
    let state = state.clone();
    handle.spawn(async move {
        let cid = state.company_id();
        match state.order_service.find_all(cid).await {
            Ok(orders) => {
                state.alarm_watcher.seed(orders.iter());
                tracing::debug!(
                    "alarme: watcher seeded com {} pedido(s) pendente(s) existentes",
                    orders.iter().filter(|o| matches!(o.status, letaf_core::order::model::OrderStatus::Pending)).count(),
                );
            }
            Err(e) => tracing::warn!("alarme: falha ao fazer seed do watcher: {e}"),
        }
    });
}

/// Task tokio que escuta `alarm_signal.notified()` e, a cada
/// notificação, mexe nas propriedades do `MainWindow` via
/// `slint::invoke_from_event_loop` (Slint é single-threaded).
///
/// O contador `alarm-pending-count` é recalculado consultando o DB
/// — assim o badge do modal mostra "Você tem 3 pedidos pendentes",
/// não só "1 novo" (operador pode ter perdido alarmes anteriores).
fn setup_alarm_observer(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
) {
    let ui_weak = ui.as_weak();
    let signal = state.alarm_signal.clone();
    let state = state.clone();
    handle.spawn(async move {
        loop {
            signal.notified().await;
            // Conta pendentes em paralelo ao set/ativação do modal —
            // se falhar (DB offline), seguimos com 0 para não
            // bloquear o aviso visual.
            let pending = match state.order_service.find_all(state.company_id()).await {
                Ok(os) => os.iter()
                    .filter(|o| matches!(o.status, letaf_core::order::model::OrderStatus::Pending))
                    .count() as i32,
                Err(_) => 0,
            };
            let ui_weak = ui_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    ui.set_alarm_pending_count(pending);
                    ui.set_alarm_active(true);
                    ui.set_alarm_visible(true);
                }
            });
        }
    });
}

/// Callback "Ver pedidos" do modal — para o som, fecha o modal,
/// limpa o estado de alarme e navega para a aba Pedidos.
fn setup_alarm_acknowledge(ui: &MainWindow, state: &DesktopState) {
    let ui_weak = ui.as_weak();
    let player = state.alarm_player.clone();
    ui.on_alarm_acknowledge(move || {
        player.stop();
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_alarm_active(false);
            ui.set_alarm_visible(false);
            ui.set_active_tab(SharedString::from("orders"));
            // Força refresh para o operador ver o novo pedido logo.
            ui.invoke_refresh_orders();
        }
    });
}

/// Callback "Fechar" do modal — apenas fecha visualmente. O som
/// continua e, em 5 s, o Timer Slint reabre o modal (regra do
/// usuário: "enquanto não clicar no botão, não vai parar de
/// avisar").
fn setup_alarm_dismiss(ui: &MainWindow, _state: &DesktopState) {
    let ui_weak = ui.as_weak();
    ui.on_alarm_dismiss(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_alarm_visible(false);
        }
    });
}
