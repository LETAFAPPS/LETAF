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
    /// Entidades cujo PULL falhou no último ciclo (403 de permissão, 500,
    /// erro de decode). Sem isto, uma entidade podia estar congelada há
    /// horas com a UI mostrando "Sincronizado" — a falha só existia no
    /// log (§7.6).
    pub pull_failed_count: u32,
    /// Registros que o servidor mandou e este binário não soube ler
    /// (versão mais nova do lado de lá). Foram PULADOS: o resto da página
    /// entrou, mas o operador precisa saber que falta dado.
    pub poison_count: u32,
    /// Diferença do relógio local para o do servidor, em segundos (positivo =
    /// esta máquina adiantada). A resolução de conflito é last-write-wins
    /// sobre `updated_at` carimbado pelo cliente (§7.7): com o relógio errado,
    /// este terminal sobrescreve — ou perde — alterações dos outros SEM
    /// nenhum erro aparecer. Acima de `CLOCK_SKEW_TOLERANCE_SECS` a UI avisa.
    pub clock_skew_seconds: i64,
}

/// Tolerância antes de considerar o relógio fora de hora. Dois minutos
/// absorvem a imprecisão do header `Date` (resolução de 1 s) e a latência da
/// rede, sem deixar passar um relógio de fato errado.
pub const CLOCK_SKEW_TOLERANCE_SECS: i64 = 120;

impl Default for SyncStatus {
    fn default() -> Self {
        Self {
            phase: SyncPhase::Idle,
            online: true,
            last_sync_at: None,
            pending_count: 0,
            rejected_count: 0,
            pull_failed_count: 0,
            poison_count: 0,
            clock_skew_seconds: 0,
        }
    }
}

/// Resultado consolidado de um ciclo de sync, publicado de uma vez.
#[derive(Debug, Clone, Copy)]
pub struct CycleOutcome {
    pub online: bool,
    pub last_sync_at: NaiveDateTime,
    pub pending_count: u32,
    pub rejected_count: u32,
    pub pull_failed_count: u32,
    pub poison_count: u32,
    pub clock_skew_seconds: i64,
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
    pub fn mark_finished(&self, r: CycleOutcome) {
        if let Ok(mut g) = self.0.write() {
            g.online = r.online;
            // Erro quando a rede caiu, quando há dado rejeitado (4xx)
            // preso, quando alguma entidade não conseguiu ser PUXADA ou
            // quando veio registro ilegível — todas são situações em que
            // os dois bancos estão divergindo e o operador precisa ver.
            g.phase = if !r.online
                || r.rejected_count > 0
                || r.pull_failed_count > 0
                || r.poison_count > 0
            {
                SyncPhase::Error
            } else {
                SyncPhase::Idle
            };
            if r.online {
                g.last_sync_at = Some(r.last_sync_at);
            }
            g.pending_count = r.pending_count;
            g.rejected_count = r.rejected_count;
            g.pull_failed_count = r.pull_failed_count;
            g.poison_count = r.poison_count;
            g.clock_skew_seconds = r.clock_skew_seconds;
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
