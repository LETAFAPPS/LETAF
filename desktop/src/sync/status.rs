use std::sync::{Arc, RwLock};

use chrono::NaiveDateTime;

/// Fase atual do SyncWorker.
///
/// Regras aplicadas (AI_RULES.md §7):
/// - `Idle`: nenhum ciclo em andamento
/// - `Syncing`: push/pull em execução
/// - `Error`: último ciclo falhou em algum push/pull (rede indisponível)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncPhase {
    Idle,
    Syncing,
    Error,
}

/// Snapshot do estado do SyncWorker compartilhado entre worker e UI.
///
/// Regras aplicadas (AI_RULES.md §7, §8):
/// - Estado leve, somente leitura/escrita rápida (sem queries de banco aqui)
/// - Worker atualiza durante `run_cycle`; UI lê via timer
/// - `pending_count` é a soma de `find_unsynced` em todos os domínios
#[derive(Debug, Clone)]
pub struct SyncStatus {
    pub phase: SyncPhase,
    pub online: bool,
    pub last_sync_at: Option<NaiveDateTime>,
    pub pending_count: u32,
    /// Registros REJEITADOS pelo servidor com erro de cliente (4xx) no último
    /// ciclo — dado que nunca vai subir sem intervenção (ex.: permissão
    /// insuficiente, dado inválido). Diferente de `pending_count`, que pode ser
    /// só "ainda não enviado". `> 0` acende o estado de erro para o operador ver
    /// que há dado preso, em vez de um "Sincronizado" enganoso (§7.6).
    pub rejected_count: u32,
}

impl Default for SyncStatus {
    fn default() -> Self {
        Self {
            phase: SyncPhase::Idle,
            online: true,
            last_sync_at: None,
            pending_count: 0,
            rejected_count: 0,
        }
    }
}

/// Handle thread-safe para compartilhar o `SyncStatus` entre worker e UI.
///
/// Usa `std::sync::RwLock` em vez de `tokio::sync::RwLock` para permitir
/// leitura síncrona do event loop Slint (que não pode aguardar `await`).
#[derive(Clone, Default)]
pub struct SyncStatusHandle(Arc<RwLock<SyncStatus>>);

impl SyncStatusHandle {
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot do estado atual (clone barato — struct pequena).
    pub fn snapshot(&self) -> SyncStatus {
        self.0.read().map(|g| g.clone()).unwrap_or_default()
    }

    /// Sinaliza início do ciclo de sync.
    pub fn mark_syncing(&self) {
        if let Ok(mut g) = self.0.write() {
            g.phase = SyncPhase::Syncing;
        }
    }

    /// Sinaliza fim do ciclo, com resultado consolidado.
    pub fn mark_finished(
        &self,
        online: bool,
        last_sync_at: NaiveDateTime,
        pending_count: u32,
        rejected_count: u32,
    ) {
        if let Ok(mut g) = self.0.write() {
            g.online = online;
            // Erro quando a rede caiu OU quando há dado rejeitado (4xx) preso —
            // ambos são situações que o operador precisa enxergar.
            g.phase = if !online || rejected_count > 0 {
                SyncPhase::Error
            } else {
                SyncPhase::Idle
            };
            if online {
                g.last_sync_at = Some(last_sync_at);
            }
            g.pending_count = pending_count;
            g.rejected_count = rejected_count;
        }
    }

    /// Atualiza apenas o contador de pendentes (chamado fora do ciclo se preciso).
    pub fn set_pending(&self, pending_count: u32) {
        if let Ok(mut g) = self.0.write() {
            g.pending_count = pending_count;
        }
    }

    /// Atualiza apenas o flag `online` (heartbeat do HealthChecker).
    ///
    /// Regras aplicadas (AI_RULES.md §7):
    /// - Mantém os demais campos intactos para não conflitar com o ciclo de sync.
    /// - Combinado com `phase`, alimenta o rótulo da UI:
    ///   Idle + offline → "offline"; Syncing → "sincronizando…"; Error → "erro…".
    pub fn set_online(&self, online: bool) {
        if let Ok(mut g) = self.0.write() {
            g.online = online;
        }
    }
}
