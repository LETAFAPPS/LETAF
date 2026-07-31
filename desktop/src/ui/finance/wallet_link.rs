//! Ponte Financeiro → carteira do cliente.
//!
//! Uma conta a RECEBER lançada com um cliente vinculado é uma dívida
//! dele: ao criar, a carteira é debitada; ao receber (total ou
//! parcial), creditada de volta; ao cancelar/excluir uma conta aberta,
//! o débito é estornado. Assim o saldo da carteira e o Financeiro
//! contam a mesma história.
//!
//! A conta AUTOMÁTICA do fiado (`[fiado-auto]`) fica de fora: ela já é
//! o espelho do saldo negativo, e debitar de novo dobraria a dívida.
//!
//! Falha na carteira só loga (igual ao espelho do fiado): o lançamento
//! financeiro é a fonte de verdade e não é desfeito por erro do reflexo.

use rust_decimal::Decimal;
use uuid::Uuid;

use letaf_core::finance::model::{
    FinanceEntry, FinanceKind, FinanceRecurrence, FinanceStatus, PartyType,
};
use letaf_core::finance::service::FIADO_AUTO_TAG;

use crate::context::DesktopState;

/// `Some(customer_id)` quando o lançamento deve refletir na carteira:
/// conta a receber, de um cliente, que não seja a do fiado automático.
fn linked_customer(entry: &FinanceEntry) -> Option<Uuid> {
    if entry.kind != FinanceKind::Receivable
        || entry.party_type != PartyType::Customer
        || entry.notes.as_deref() == Some(FIADO_AUTO_TAG)
        // Recorrência é COMPROMISSO futuro, não dívida assumida: uma
        // mensalidade de R$ 200 gera 12 ocorrências e debitar as 12 no
        // ato deixaria o cliente com −2.400 e bloqueado no PDV por algo
        // que ainda nem venceu. O mesmo recorte vale no espelho do fiado
        // (`open_customer_receivables_total`).
        || entry.recurrence != FinanceRecurrence::Once
    {
        return None;
    }
    entry.party_id
}

/// Carteira do cliente, criando-a se ainda não existir (limite zero —
/// o débito da conta não depende de limite de fiado).
async fn account_id_for(state: &DesktopState, customer_id: Uuid) -> Option<Uuid> {
    let cid = state.company_id();
    match state
        .wallet_service
        .find_account_by_customer(cid, customer_id)
        .await
    {
        Ok(Some(account)) => Some(account.base.id),
        Ok(None) => state
            .wallet_service
            .open_account(cid, customer_id, Decimal::ZERO)
            .await
            .map(|a| a.base.id)
            .map_err(|e| tracing::warn!("financeiro→carteira: não abriu a carteira: {e}"))
            .ok(),
        Err(e) => {
            tracing::warn!("financeiro→carteira: não carregou a carteira: {e}");
            None
        }
    }
}

/// Debita na carteira o valor de um GRUPO de contas a receber recém-criado
/// (lançamento simples, parcelado ou recorrente).
///
/// O débito é a SOMA das entradas: é exatamente o que
/// `open_customer_receivables_total` vai descontar do espelho do fiado.
/// Debitar só a 1ª parcela deixava a carteira menor que a dívida e o
/// espelho apagava fiado real do Financeiro.
pub(crate) async fn charge_group(state: &DesktopState, entries: &[FinanceEntry]) {
    let Some(head) = entries.first() else { return };
    let Some(customer_id) = linked_customer(head) else { return };
    let total: Decimal = entries
        .iter()
        .filter(|e| !e.status.is_settled() && e.status != FinanceStatus::Cancelled)
        .map(|e| e.amount)
        .sum();
    apply_charge(state, customer_id, total, &head.description).await;
}

/// Credita a baixa (total ou parcial) de uma conta a receber.
pub(crate) async fn settle(state: &DesktopState, entry: &FinanceEntry, amount: Decimal) {
    let Some(customer_id) = linked_customer(entry) else { return };
    let amount = amount.min(entry.amount);
    apply_settle(
        state,
        customer_id,
        amount,
        &format!("Recebimento — {}", entry.description),
    )
    .await;
}

/// Estorna o débito de uma conta a receber ABERTA que foi cancelada ou
/// excluída (o cliente deixa de dever esse valor).
pub(crate) async fn revert(state: &DesktopState, entry: &FinanceEntry) {
    let Some(customer_id) = linked_customer(entry) else { return };
    if entry.status.is_settled() || entry.status == FinanceStatus::Cancelled {
        return;
    }
    apply_settle(
        state,
        customer_id,
        entry.amount,
        &format!("Estorno — {}", entry.description),
    )
    .await;
}

/// Reflete a EDIÇÃO de uma conta aberta: estorna o que valia antes e
/// debita o novo valor/cliente (cobre troca de cliente e de valor).
pub(crate) async fn update(state: &DesktopState, before: &FinanceEntry, after: &FinanceEntry) {
    let old_customer = linked_customer(before).filter(|_| {
        !before.status.is_settled() && before.status != FinanceStatus::Cancelled
    });
    let new_customer = linked_customer(after);

    // Mesmo cliente: aplica só a diferença de valor.
    if let Some(customer_id) = old_customer.filter(|_| old_customer == new_customer) {
        let delta = after.amount - before.amount;
        if delta > Decimal::ZERO {
            apply_charge(state, customer_id, delta, &after.description).await;
        } else if delta < Decimal::ZERO {
            apply_settle(
                state,
                customer_id,
                -delta,
                &format!("Ajuste — {}", after.description),
            )
            .await;
        }
        return;
    }

    if let Some(customer_id) = old_customer {
        apply_settle(
            state,
            customer_id,
            before.amount,
            &format!("Estorno — {}", before.description),
        )
        .await;
    }
    if let Some(customer_id) = new_customer {
        apply_charge(state, customer_id, after.amount, &after.description).await;
    }
}

async fn apply_charge(
    state: &DesktopState,
    customer_id: Uuid,
    amount: Decimal,
    description: &str,
) {
    if amount <= Decimal::ZERO {
        return;
    }
    let Some(account_id) = account_id_for(state, customer_id).await else { return };
    match state
        .wallet_service
        .charge_receivable(
            state.company_id(),
            account_id,
            amount,
            Some(description.to_string()),
        )
        .await
    {
        Ok((account, _)) => super::super::wallet::sync_fiado_to_finance(state, &account).await,
        Err(e) => tracing::warn!("financeiro→carteira: débito não aplicado: {e}"),
    }
}

async fn apply_settle(
    state: &DesktopState,
    customer_id: Uuid,
    amount: Decimal,
    description: &str,
) {
    if amount <= Decimal::ZERO {
        return;
    }
    let Some(account_id) = account_id_for(state, customer_id).await else { return };
    match state
        .wallet_service
        .settle_receivable(
            state.company_id(),
            account_id,
            amount,
            Some(description.to_string()),
        )
        .await
    {
        Ok((account, _)) => super::super::wallet::sync_fiado_to_finance(state, &account).await,
        Err(e) => tracing::warn!("financeiro→carteira: crédito não aplicado: {e}"),
    }
}
