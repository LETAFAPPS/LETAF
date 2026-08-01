//! Reconciliação de cobranças PIX avulsas (AI_RULES.md §1, §3, §11).
//!
//! Antes, quem percebia que um PIX foi pago era o LAÇO DE POLLING DA UI do
//! desktop: enquanto o modal do QR Code ficava aberto, ele chamava `/refresh`
//! e, ao ver `Paid`, dava baixa na fatura. Fechar o modal — ou o terminal cair
//! — significava PIX pago e fatura em aberto para sempre.
//!
//! Isso também punha decisão de negócio no frontend (§3/§11: o cliente é
//! burro e não-confiável). Aqui a decisão volta para o servidor: um tick
//! periódico consulta o gateway sobre as cobranças pendentes e quita a fatura.
//! O desktop continua com o polling só para dar feedback imediato na tela; se
//! ele sumir, o resultado é o mesmo, só que alguns segundos depois.
//!
//! `mark_invoice_paid` é idempotente, então os dois caminhos convivem sem
//! quitar duas vezes.

use std::time::Duration;

use letaf_core::payment_gateway::model::{ChargeStatus, PaymentCharge};
use letaf_core::payment_gateway::service::CHARGE_RECONCILE_INTERVAL_SECS;

use crate::context::AppState;

/// Spawn do laço. Chamado uma vez no boot, antes do axum. Sem gateway
/// configurado o laço nem sobe — não há o que consultar.
pub fn start_charge_reconcile_loop(state: AppState) {
    if state.payment_service.is_none() {
        return;
    }
    tokio::spawn(async move {
        // Mesmo respiro do billing: deixa o pool do PG esquentar.
        tokio::time::sleep(Duration::from_secs(20)).await;
        let mut interval =
            tokio::time::interval(Duration::from_secs(CHARGE_RECONCILE_INTERVAL_SECS));
        interval.tick().await;
        loop {
            interval.tick().await;
            let quitadas = tick_once(&state).await;
            if quitadas > 0 {
                tracing::info!("Reconcile de cobranças: {quitadas} fatura(s) quitada(s)");
            }
        }
    });
}

/// Um tick: consulta o gateway sobre cada cobrança pendente e dá baixa nas
/// que foram pagas. Devolve quantas faturas quitou.
///
/// Resiliente por cobrança (§7.6): gateway fora do ar ou uma cobrança
/// problemática não podem impedir as demais de reconciliar.
async fn tick_once(state: &AppState) -> u32 {
    let Some(svc) = state.payment_service.as_ref() else {
        return 0;
    };
    let pendentes = match svc.find_pending(chrono::Utc::now().naive_utc()).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Reconcile de cobranças: não listou pendentes: {e}");
            return 0;
        }
    };

    let mut quitadas = 0;
    for pendente in pendentes {
        let cid = pendente.base.company_id;
        let atual = match svc.refresh_status(cid, pendente.base.id).await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Reconcile da cobrança {}: {e}", pendente.base.id);
                continue;
            }
        };
        if settle_if_paid(state, &atual).await {
            quitadas += 1;
        }
    }
    quitadas
}

/// Dá baixa na fatura quando a cobrança está PAGA. Idempotente — chamável
/// pelo tick e pela rota `/refresh` sem quitar duas vezes.
///
/// Devolve `true` se quitou. Cobrança avulsa (sem `invoice_id`) só muda de
/// status: não há fatura a quitar.
pub async fn settle_if_paid(state: &AppState, charge: &PaymentCharge) -> bool {
    if !matches!(charge.status, ChargeStatus::Paid) {
        return false;
    }
    let Some(invoice_id) = charge.invoice_id else {
        return false;
    };
    match state
        .subscription_service
        .mark_invoice_paid(charge.base.company_id, invoice_id, charge.paid_at)
        .await
    {
        Ok(_) => true,
        Err(e) => {
            tracing::warn!("Baixa da fatura {invoice_id} falhou: {e}");
            false
        }
    }
}
