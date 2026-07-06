// ── Alarme de novos pedidos ─────────────────────────────────────────
//
// Responsável por avisar o operador quando um pedido novo (com status
// `Pending`) chega via sincronização. O alarme é dividido em dois
// pequenos serviços, cada um com uma responsabilidade única
// (AI_RULES.md §8):
//
// - `AlarmPlayer` (`player.rs`) — toca um beep em loop fora do event
//   loop Slint, comunicando com a thread de áudio via canal mpsc.
//   Continua tocando enquanto a janela estiver minimizada.
//
// - `AlarmWatcher` (`watcher.rs`) — guarda em memória o conjunto de
//   IDs de pedidos pendentes já "vistos". Usado pelo `SyncWorker` para
//   decidir se um pedido recém-puxado é realmente NOVO (e portanto
//   deve disparar o alarme) ou se é um pendente antigo.
//
// Toda a UI (modal, som, timer de reabertura) é orquestrada pelo
// MainWindow Slint — Rust apenas alimenta as propriedades e expõe
// callbacks. Core/server não conhecem este módulo (AI_RULES.md §1).

mod player;
mod watcher;

pub use player::AlarmPlayer;
pub use watcher::AlarmWatcher;
