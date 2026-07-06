use std::collections::HashSet;
use std::sync::Mutex;

use letaf_core::order::model::{Order, OrderStatus};
use uuid::Uuid;

/// Mantém o conjunto de pedidos já "vistos" durante esta sessão do
/// desktop. Usado pelo `SyncWorker` para diferenciar pedidos
/// genuinamente novos (chegaram via sync após o boot) de pendentes
/// antigos que estavam no banco quando o app abriu.
///
/// Estado é totalmente em memória — propositadamente: ao reabrir o
/// app, o `seed()` reidrata o set com todos os pendentes existentes
/// no SQLite, evitando alarme em pedidos que o operador "viu" mas não
/// processou.
pub struct AlarmWatcher {
    /// IDs de pedidos já notificados nesta sessão. Atômico por
    /// `Mutex<HashSet>` — operações são raras (max 1×/30s pelo
    /// SyncWorker), então contenção não é problema.
    seen: Mutex<HashSet<Uuid>>,
}

impl AlarmWatcher {
    pub fn new() -> Self {
        Self { seen: Mutex::new(HashSet::new()) }
    }

    /// Pré-popula o set com IDs de pendentes existentes no boot.
    /// Sem isso, todo pedido pendente do banco seria considerado
    /// "novo" no primeiro ciclo após o app iniciar e dispararia
    /// alarme indevido — mau UX se o operador acaba de abrir o app.
    pub fn seed<'a, I: IntoIterator<Item = &'a Order>>(&self, orders: I) {
        let mut seen = self.seen.lock().expect("alarm watcher mutex poisoned");
        for o in orders {
            if matches!(o.status, OrderStatus::Pending) {
                seen.insert(o.base.id);
            }
        }
    }

    /// Registra um pedido recém-puxado. Devolve `true` se ele é
    /// elegível para alarme (status `Pending` E ainda não estava no
    /// set). O caller (SyncWorker) usa o retorno para decidir se
    /// dispara `AlarmPlayer::start()` + abre o modal.
    pub fn note(&self, order: &Order) -> bool {
        if !matches!(order.status, OrderStatus::Pending) {
            // Quando um pedido sai de Pending (operador confirmou,
            // por exemplo), removemos do set — assim, se um dia ele
            // voltar para Pending (cenário hipotético), dispara de
            // novo. Custo é mínimo (lookup O(1)).
            self.seen.lock().ok().map(|mut s| s.remove(&order.base.id));
            return false;
        }
        let mut seen = match self.seen.lock() {
            Ok(g) => g,
            Err(_) => return false,
        };
        seen.insert(order.base.id) // `insert` devolve true se era novo
    }
}

impl Default for AlarmWatcher {
    fn default() -> Self { Self::new() }
}
