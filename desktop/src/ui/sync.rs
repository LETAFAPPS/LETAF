use std::sync::Arc;
use std::time::Duration;

use chrono::NaiveDateTime;
use slint::{ComponentHandle, SharedString, Timer, TimerMode};
use tokio::sync::RwLock;

use crate::MainWindow;
use crate::sync::status::{SyncPhase, SyncStatusHandle};

/// Intervalo de polling do status — equilibra atualização visível e custo de CPU.
const POLL_INTERVAL_MS: u64 = 1500;

/// Inicia o timer Slint que reflete o `SyncStatus` nas propriedades da UI.
///
/// Regras aplicadas (AI_RULES.md §3, §7, §8, §11):
/// - UI sem lógica de negócio: apenas lê snapshots e formata texto.
/// - Timer roda no event loop Slint (não bloqueia).
/// - Se o `auth_token` for invalidado pelo SyncWorker (401 do servidor) enquanto
///   a UI ainda está em `logged_in=true`, força logout para evitar estado
///   inconsistente — ver `detect_session_invalidation`.
/// - O Timer precisa viver até o fim do programa; usamos `mem::forget` para
///   evitar que o destrutor pare o timer ao final desta função.
pub(super) fn start_sync_status_timer(
    ui: &MainWindow,
    status: SyncStatusHandle,
    auth_token: Arc<RwLock<Option<String>>>,
) {
    let timer = Timer::default();
    let ui_weak = ui.as_weak();

    let tick = move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        let snapshot = status.snapshot();
        let now = chrono::Utc::now().naive_utc();

        ui.set_sync_online(snapshot.online);
        ui.set_sync_pending_count(snapshot.pending_count as i32);
        ui.set_sync_status_label(SharedString::from(phase_label(snapshot.phase, snapshot.online)));
        ui.set_sync_last_label(SharedString::from(format_last_sync(snapshot.last_sync_at, now)));

        detect_session_invalidation(&ui, &auth_token);
    };

    timer.start(TimerMode::Repeated, Duration::from_millis(POLL_INTERVAL_MS), tick);
    std::mem::forget(timer);
}

/// Força logout se o token foi invalidado pelo worker enquanto a UI continua logada.
///
/// Cenário típico: SyncWorker recebe 401, limpa `auth_token` (vira `None`),
/// mas a UI ainda mostra `logged_in = true`. Sem este check, o usuário
/// continuaria vendo telas vazias até interagir e perceber o erro.
fn detect_session_invalidation(ui: &MainWindow, auth_token: &RwLock<Option<String>>) {
    if !ui.get_logged_in() {
        return;
    }
    // `try_read` não bloqueia o event loop. Em caso de lock concorrente,
    // simplesmente pulamos esta verificação (próximo tick checa de novo).
    let token_present = match auth_token.try_read() {
        Ok(guard) => guard.is_some(),
        Err(_) => return,
    };
    if token_present {
        return;
    }
    ui.set_logged_in(false);
    ui.set_login_status(SharedString::from(
        "Sua sessão expirou. Faça login novamente.",
    ));
    ui.set_status_message(SharedString::from("Sessão expirada"));
}

/// Converte a fase + status de rede em rótulo para a UI.
fn phase_label(phase: SyncPhase, online: bool) -> &'static str {
    match phase {
        SyncPhase::Syncing => "Sincronizando…",
        SyncPhase::Error   => "Erro de sincronização",
        SyncPhase::Idle    => if online { "Sincronizado" } else { "Offline" },
    }
}

/// Formata a diferença `now - last` em texto relativo curto.
///
/// Regras aplicadas (AI_RULES.md §8): função pura, sem efeitos colaterais.
fn format_last_sync(last: Option<NaiveDateTime>, now: NaiveDateTime) -> String {
    let Some(last) = last else { return "—".to_string() };
    let secs = (now - last).num_seconds();
    if secs < 0 {
        return "agora".to_string();
    }
    match secs {
        0..=4         => "agora".to_string(),
        5..=59        => format!("há {secs} s"),
        60..=3599     => format!("há {} min", secs / 60),
        3600..=86_399 => format!("há {} h", secs / 3600),
        _             => format!("há {} d", secs / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn ts(secs_back: i64) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2030, 1, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
            - chrono::Duration::seconds(secs_back)
    }

    #[test]
    fn relative_labels() {
        let now = NaiveDate::from_ymd_opt(2030, 1, 1).unwrap().and_hms_opt(12, 0, 0).unwrap();
        assert_eq!(format_last_sync(None, now),              "—");
        assert_eq!(format_last_sync(Some(ts(0)), now),       "agora");
        assert_eq!(format_last_sync(Some(ts(3)), now),       "agora");
        assert_eq!(format_last_sync(Some(ts(30)), now),      "há 30 s");
        assert_eq!(format_last_sync(Some(ts(120)), now),     "há 2 min");
        assert_eq!(format_last_sync(Some(ts(7_200)), now),   "há 2 h");
        assert_eq!(format_last_sync(Some(ts(180_000)), now), "há 2 d");
    }

    #[test]
    fn phase_labels_match_states() {
        assert_eq!(phase_label(SyncPhase::Syncing, true),  "Sincronizando…");
        assert_eq!(phase_label(SyncPhase::Syncing, false), "Sincronizando…");
        assert_eq!(phase_label(SyncPhase::Error,   true),  "Erro de sincronização");
        assert_eq!(phase_label(SyncPhase::Idle,    true),  "Sincronizado");
        assert_eq!(phase_label(SyncPhase::Idle,    false), "Offline");
    }
}
