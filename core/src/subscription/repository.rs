use async_trait::async_trait;
use chrono::NaiveDateTime;
use uuid::Uuid;

use super::model::{Invoice, Subscription};
use crate::error::CoreError;

/// Trait de acesso a dados para Subscription + Invoice (mesma camada
/// porque o ciclo de vida é acoplado: fatura nasce da assinatura).
///
/// Regras aplicadas (AI_RULES.md §10): acesso ao banco apenas via
/// repository, abstraído por trait — PG (server) e SQLite (desktop).
#[async_trait]
pub trait SubscriptionRepository: Send + Sync {
    async fn find_current(&self, company_id: Uuid) -> Result<Option<Subscription>, CoreError>;

    /// Assinatura atual de VÁRIAS empresas numa só query (painel super-admin,
    /// §13 — evita N+1). Retorna no máximo uma por company. Default: laço de
    /// `find_current` (desktop não usa); o servidor sobrescreve com `DISTINCT ON`.
    async fn find_current_for_companies(
        &self,
        company_ids: &[Uuid],
    ) -> Result<Vec<Subscription>, CoreError> {
        let mut out = Vec::new();
        for &id in company_ids {
            if let Some(s) = self.find_current(id).await? {
                out.push(s);
            }
        }
        Ok(out)
    }
    /// Busca uma assinatura pelo seu próprio id, independente de
    /// status/`next_charge_date` (usado no fluxo de cobrança).
    async fn find_subscription_by_id(
        &self,
        id: Uuid,
    ) -> Result<Option<Subscription>, CoreError>;
    async fn create_subscription(&self, s: &Subscription) -> Result<(), CoreError>;
    async fn update_subscription(&self, s: &Subscription) -> Result<(), CoreError>;

    async fn find_invoices(&self, company_id: Uuid) -> Result<Vec<Invoice>, CoreError>;
    async fn create_invoice(&self, inv: &Invoice) -> Result<(), CoreError>;
    async fn update_invoice(&self, inv: &Invoice) -> Result<(), CoreError>;

    /// Lista de assinaturas ativas com `next_charge_date <= today`.
    /// Usado pelo billing loop para descobrir quem precisa ser cobrado.
    /// **Não respeita company_id** porque o loop roda no server e
    /// percorre todas as empresas; cada `Subscription` já carrega seu
    /// próprio `company_id`.
    async fn find_due_subscriptions(
        &self,
        today: chrono::NaiveDate,
    ) -> Result<Vec<Subscription>, CoreError>;

    /// Assinaturas cuja invoice em aberto mais antiga venceu há >
    /// `grace_days` dias. Candidatas a `Overdue`.
    async fn find_overdue_candidates(
        &self,
        today: chrono::NaiveDate,
        grace_days: i64,
    ) -> Result<Vec<Subscription>, CoreError>;

    /// Busca a assinatura por `gateway_subscription_id`. Usada no server
    /// ao processar notificações do gateway (webhook), que só trazem o
    /// ID remoto. Global (sem company_id) — cada `Subscription` carrega
    /// o seu. Retorna `None` no desktop (não recebe webhooks).
    async fn find_by_gateway_subscription_id(
        &self,
        gateway_subscription_id: &str,
    ) -> Result<Option<Subscription>, CoreError>;

    /// Busca a assinatura pelo `pix_auto_rec_id` (idRec). Usada no
    /// server ao processar o webhook do Pix Automático.
    async fn find_by_pix_auto_rec_id(
        &self,
        rec_id: &str,
    ) -> Result<Option<Subscription>, CoreError>;

    /// Verifica se já existe uma invoice para essa subscription
    /// emitida em (year, month). Usado para garantir idempotência do
    /// billing loop (não duplica cobrança quando roda 2× no mesmo mês).
    async fn find_invoice_in_month(
        &self,
        subscription_id: Uuid,
        year: i32,
        month: u32,
    ) -> Result<Option<Invoice>, CoreError>;

    async fn find_unsynced_subscriptions(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<Subscription>, CoreError>;
    async fn find_unsynced_invoices(&self, company_id: Uuid) -> Result<Vec<Invoice>, CoreError>;
    async fn mark_subscription_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError>;
    async fn mark_invoice_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError>;

    /// §7.7 — last-write-wins via `updated_at`.
    async fn sync_upsert_subscription(&self, s: &Subscription) -> Result<(), CoreError>;
    async fn sync_upsert_invoice(&self, inv: &Invoice) -> Result<(), CoreError>;

    async fn find_subscriptions_updated_since(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
    ) -> Result<Vec<Subscription>, CoreError>;
    async fn find_invoices_updated_since(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
    ) -> Result<Vec<Invoice>, CoreError>;
    /// Página do pull de faturas por keyset `(updated_at, id)` (default delega
    /// ao acima).
    async fn find_invoices_updated_since_paged(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
        _after_id: Uuid,
        _limit: i64,
    ) -> Result<Vec<Invoice>, CoreError> {
        self.find_invoices_updated_since(company_id, since).await
    }
}
