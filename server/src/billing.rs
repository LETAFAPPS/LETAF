//! Loop de cobrança recorrente (Fase 14B).
//!
//! Regras aplicadas (AI_RULES.md §1, §8, §11):
//! - Lógica de "quem cobrar quando" vive no `SubscriptionService` do
//!   core; este módulo é só o ticker assíncrono que chama o service.
//! - Não bloqueia o `axum` serve — roda em `tokio::spawn` à parte.
//! - Resiliente: erro numa cobrança não derruba o loop (loga e segue).
//! - Idempotente: `record_charge_attempt` no service evita duplicar
//!   invoice no mesmo mês mesmo que o tick rode 2× no mesmo dia.

use std::time::Duration;

use chrono::Local;

use letaf_core::subscription::service::BILLING_TICK_INTERVAL_SECS;

use crate::context::AppState;

/// Spawn do loop. Chamado uma vez no boot do server, antes do axum.
/// O server segue subindo mesmo sem gateway configurado — o loop só
/// emite cobranças quando `payment_service` está presente.
pub fn start_billing_loop(state: AppState) {
    tokio::spawn(async move {
        // Pequeno delay antes do primeiro tick para deixar o server
        // estabilizar (pool de PG quente, conexões prontas).
        tokio::time::sleep(Duration::from_secs(15)).await;
        let mut interval =
            tokio::time::interval(Duration::from_secs(BILLING_TICK_INTERVAL_SECS));
        // `interval` dispara imediatamente no primeiro `tick`; pulamos
        // para alinhar com a janela de delay acima.
        interval.tick().await;
        loop {
            interval.tick().await;
            if let Err(e) = tick_once(&state).await {
                tracing::warn!("billing tick falhou: {e}");
            }
        }
    });
}

async fn tick_once(state: &AppState) -> Result<(), String> {
    let today = Local::now().date_naive();

    // 1) Assinaturas a cobrar hoje.
    let due = state
        .subscription_service
        .find_due_subscriptions(today)
        .await
        .map_err(|e| format!("find_due: {e}"))?;
    // Assinaturas com cartão recorrente são cobradas pelo próprio
    // gateway (motor de assinaturas da Efi) — não emitimos nada.
    // As notificações chegam via webhook (`/webhooks/efi`).
    //
    // Pix Automático: emitimos um `cobr` por ciclo (o banco do pagador
    // debita sozinho). Webhook em `/webhooks/efi/pix` confirma.
    for sub in due.iter().filter(|s| s.has_active_pix_auto()) {
        if let Some(pix_auto) = state.pix_auto.as_ref() {
            if let Err(e) = pix_auto.charge_cycle(sub, today).await {
                tracing::warn!("cobr Pix Automático falhou para {}: {e}", sub.base.id);
            }
        }
    }

    // PIX manual: geramos a cobrança imediata (operador paga o QR).
    let pix_due: Vec<_> = due
        .iter()
        .filter(|s| !s.has_active_card() && !s.has_active_pix_auto())
        .collect();
    if !pix_due.is_empty() {
        tracing::info!("billing tick: {} assinatura(s) a cobrar (PIX)", pix_due.len());
    }
    for sub in &pix_due {
        if let Err(e) = charge_subscription(state, sub.base.id, today).await {
            tracing::warn!("cobrança falhou para subscription {}: {e}", sub.base.id);
        }
    }

    // 2) Assinaturas em atraso (status → Overdue).
    let overdue = state
        .subscription_service
        .find_overdue_candidates(today)
        .await
        .map_err(|e| format!("overdue: {e}"))?;
    for sub in &overdue {
        if let Err(e) = state.subscription_service.mark_overdue(sub.base.id).await {
            tracing::warn!("mark_overdue falhou para {}: {e}", sub.base.id);
        }
    }
    Ok(())
}

async fn charge_subscription(
    state: &AppState,
    subscription_id: uuid::Uuid,
    today: chrono::NaiveDate,
) -> Result<(), String> {
    // Cria invoice + atualiza next_charge_date (idempotente).
    let invoice = state
        .subscription_service
        .record_charge_attempt(subscription_id, today)
        .await
        .map_err(|e| format!("record_charge_attempt: {e}"))?;
    // Quando o gateway está configurado, gera a cobrança PIX agora.
    // Sem gateway, a invoice fica Pending até o operador clicar em
    // "Pagar" manualmente — `payment_service` é Option (§11).
    let Some(payments) = state.payment_service.as_ref() else {
        return Ok(());
    };
    let description = format!("LETAF · {}", invoice.description);
    payments
        .create_pix_charge(
            invoice.base.company_id,
            Some(invoice.base.id),
            invoice.amount,
            &description,
        )
        .await
        .map_err(|e| format!("create_pix_charge: {e}"))?;
    Ok(())
}
