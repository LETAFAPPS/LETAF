use std::sync::Arc;

use slint::{ComponentHandle, SharedString};
use tokio::sync::RwLock;


use crate::MainWindow;
use crate::context::DesktopState;

/// Callback: persiste preferência de tema ao alternar.
pub(crate) fn setup_dark_mode(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
) {
    let state = state.clone();
    let handle = handle.clone();
    ui.on_dark_mode_changed(move |dark| {
        let state = state.clone();
        handle.spawn(async move {
            state.session.save_dark_mode(dark).await;
        });
    });
}

/// Callback: encerra sessão — limpa token, sessão SQLite e retorna ao login.
///
/// Regras aplicadas (AI_RULES.md §8, §11):
/// - Apaga auth_token compartilhado com SyncWorker (sem sync enquanto deslogado).
/// - Chama session.clear() para remover token do SQLite (sem re-login automático).
/// - Reseta UI para estado inicial de forma segura via invoke_from_event_loop.
pub(crate) fn setup_logout(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    auth_token: Arc<RwLock<Option<String>>>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_do_logout(move || {
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let auth_token = auth_token.clone();

        handle.spawn(async move {
            { *auth_token.write().await = None; }
            // Retoma o sync da loja (caso o próximo login seja de loja).
            state.set_sync_paused(false);
            state.session.clear().await;

            let rem_email = state.session.load_remember_email().await.unwrap_or_default();
            let has_remembered = !rem_email.is_empty();

            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                ui.set_logged_in(false);
                ui.set_login_status(SharedString::from(""));
                ui.set_login_email(SharedString::from(rem_email));
                // Senha não é mais pré-preenchida (§11): o usuário redigita.
                ui.set_login_remember_me(has_remembered);
                ui.set_active_tab(SharedString::from("products"));
            });
        });
    });
}

