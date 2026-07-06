use std::sync::Arc;

use slint::ComponentHandle;
use tokio::sync::Notify;
use uuid::Uuid;

use letaf_core::payment_method::model::PaymentMethod;

use crate::context::DesktopState;
use crate::MainWindow;

use super::card::toast;

// ── CRUD de formas de pagamento (Fase 14E) ───────────────────────
//
// Regras aplicadas (AI_RULES.md §11):
// - Toda validação no service; UI só roteia.
// - `pick_payment_method` chama `set_default` no service.
//   A opção fixa "pix-instant" não persiste — apenas alterna o
//   `payment_method` embutido na assinatura para "pix" via
//   `update_payment_method`.

pub(crate) fn setup_payment_method_crud(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    // (Botão "+ PIX" do header removido — não há mais entrada pública
    // para abrir o modal de cadastro avulso. O CRUD de PaymentMethod
    // continua disponível via API, apenas sem botão na UI.)

    // Clica numa opção do picker.
    // Opções persistidas → set_default.
    // Opção "pix-instant" (não-persistida) → muda payment_method embutido.
    let ui_weak = ui.as_weak();
    let state_pick = state.clone();
    let handle_pick = handle.clone();
    let sync_notify_pick = sync_notify.clone();
    ui.on_subscription_pick_payment_method(move |id_str| {
        let Some(ui) = ui_weak.upgrade() else { return };
        let id_string = id_str.to_string();
        ui.set_payment_picker_open(false);
        let ui_weak = ui_weak.clone();
        let state = state_pick.clone();
        let notify = sync_notify_pick.clone();
        handle_pick.spawn(async move {
            let cid = state.company_id();
            // Recorrência ativa (cartão ou Pix Automático): a seleção é o
            // gateway. Trocar exige cancelar antes (evita débito duplo).
            let current = state
                .subscription_service
                .find_current(cid)
                .await
                .ok()
                .flatten();
            let card_active = current.as_ref().map(|s| s.has_active_card()).unwrap_or(false);
            let pix_auto_active = current
                .as_ref()
                .map(|s| s.has_active_pix_auto())
                .unwrap_or(false);
            if card_active || pix_auto_active {
                let what = if card_active { "o cartão" } else { "o PIX Automático" };
                toast(
                    &ui_weak,
                    format!("Cancele {what} antes de trocar a forma de pagamento"),
                    "info",
                );
                return;
            }
            if id_string == "pix-instant" {
                let method = letaf_core::subscription::model::PaymentMethod {
                    kind: "pix".into(),
                    label: "PIX Automático".into(),
                    expiry: String::new(),
                };
                let _ = state
                    .subscription_service
                    .update_payment_method(cid, method)
                    .await;
            } else if let Ok(uuid) = Uuid::parse_str(&id_string) {
                let _ = state.payment_method_service.set_default(cid, uuid).await;
                // Sincroniza com o `payment_method` embutido para que o
                // card preto reflita imediatamente.
                if let Ok(Some(picked)) =
                    state.payment_method_service.find_by_id(cid, uuid).await
                {
                    let method = letaf_core::subscription::model::PaymentMethod {
                        kind: picked.kind.clone(),
                        label: payment_method_display_label(&picked),
                        expiry: picked.expiry.clone(),
                    };
                    let _ = state
                        .subscription_service
                        .update_payment_method(cid, method)
                        .await;
                }
            }
            notify.notify_one();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    ui.invoke_subscription_refresh();
                }
            });
        });
    });

}

/// Texto exibido no card preto para uma forma de pagamento.
fn payment_method_display_label(m: &PaymentMethod) -> String {
    match m.kind.as_str() {
        "card" => format!("{} {}", m.label, m.masked).trim().to_string(),
        _ => m.label.clone(),
    }
}

