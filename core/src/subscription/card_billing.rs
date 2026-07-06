use std::sync::Arc;

use uuid::Uuid;

use super::model::Subscription;
use super::service::SubscriptionService;
use crate::error::CoreError;
use crate::payment_gateway::card::{
    CardBillingAddress, CardCustomer, CardGateway, CardSubscriptionInput, CardSubscriptionStatus,
};

/// Orquestra o cartão recorrente: tokeniza + cria assinatura no gateway
/// e reflete o resultado na assinatura local (via [`SubscriptionService`]).
///
/// Regras aplicadas (AI_RULES.md §1, §8, §11):
/// - Toda a lógica de negócio vive aqui; rotas só convertem HTTP.
/// - O gateway concreto (Efi) entra por trait — core não conhece HTTP.
/// - A recorrência é dirigida pelo gateway; aqui só vinculamos e
///   reconciliamos as notificações.
pub struct CardBillingService {
    gateway: Arc<dyn CardGateway>,
    subscriptions: Arc<SubscriptionService>,
    /// URL pública que o gateway chama a cada cobrança (webhook).
    notification_url: String,
}

impl CardBillingService {
    pub fn new(
        gateway: Arc<dyn CardGateway>,
        subscriptions: Arc<SubscriptionService>,
        notification_url: String,
    ) -> Self {
        Self {
            gateway,
            subscriptions,
            notification_url,
        }
    }

    /// Cadastra um cartão recorrente a partir do `payment_token` gerado
    /// **no navegador** (Efi.js) — a tokenização server-side foi
    /// descontinuada pela Efi. O cartão (PAN/CVV) nunca passa por aqui;
    /// recebemos só o token + dados de exibição/antifraude.
    /// Pipeline: cria assinatura no gateway → vincula localmente.
    #[allow(clippy::too_many_arguments)]
    pub async fn subscribe_with_token(
        &self,
        company_id: Uuid,
        payment_token: String,
        brand: String,
        last4: String,
        expiry: String,
        customer: CardCustomer,
        billing_address: CardBillingAddress,
    ) -> Result<Subscription, CoreError> {
        if payment_token.trim().is_empty() {
            return Err(CoreError::Validation("payment_token ausente".into()));
        }
        let sub = self
            .subscriptions
            .find_current(company_id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Assinatura não encontrada".into()))?;
        let terms = self.subscriptions.terms(&sub);
        let card_label = card_label(&brand, &last4);

        let input = CardSubscriptionInput {
            payment_token,
            plan_name: format!("LETAF · Plano {}", terms.name),
            item_name: format!("Assinatura LETAF · {}", terms.name),
            amount_cents: to_cents(terms.amount),
            interval_months: terms.months,
            customer,
            billing_address,
            notification_url: self.notification_url.clone(),
            custom_id: sub.base.id.to_string(),
        };
        let created = self.gateway.create_card_subscription(&input).await?;

        self.subscriptions
            .bind_card(
                company_id,
                self.gateway.name().to_string(),
                created.gateway_subscription_id,
                card_label,
                expiry,
                created.status,
                created.next_charge_date,
            )
            .await
    }

    /// Cancela o cartão recorrente: encerra no gateway e desvincula
    /// localmente (volta para PIX).
    pub async fn cancel(&self, company_id: Uuid) -> Result<Subscription, CoreError> {
        let sub = self
            .subscriptions
            .find_current(company_id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Assinatura não encontrada".into()))?;
        if let Some(gsid) = sub.gateway_subscription_id.as_deref() {
            self.gateway.cancel_subscription(gsid).await?;
        }
        self.subscriptions.cancel_card(company_id).await
    }

    /// Consulta o status atual da assinatura no gateway (polling da 1ª
    /// cobrança). Não altera estado local.
    pub async fn refresh_status(
        &self,
        company_id: Uuid,
    ) -> Result<CardSubscriptionStatus, CoreError> {
        let sub = self
            .subscriptions
            .find_current(company_id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Assinatura não encontrada".into()))?;
        let gsid = sub
            .gateway_subscription_id
            .as_deref()
            .ok_or_else(|| CoreError::Validation("Nenhum cartão vinculado".into()))?;
        self.gateway.fetch_subscription_status(gsid).await
    }

    /// Processa uma notificação do gateway (webhook). Busca os eventos,
    /// localiza a assinatura por `gateway_subscription_id` e reconcilia
    /// cada cobrança. Best-effort: ignora eventos órfãos.
    pub async fn apply_notification(
        &self,
        token: &str,
        today: chrono::NaiveDate,
    ) -> Result<u32, CoreError> {
        let events = self.gateway.fetch_notification(token).await?;
        let mut applied = 0;
        for ev in events {
            let Some(sub) = self
                .subscriptions
                .find_by_gateway_subscription_id(&ev.gateway_subscription_id)
                .await?
            else {
                continue;
            };
            self.subscriptions
                .apply_card_charge(&sub, &ev.status, ev.amount, ev.paid_at, today)
                .await?;
            applied += 1;
        }
        Ok(applied)
    }
}

/// Rótulo exibível do cartão: "VISA •••• 4242".
fn card_label(brand: &str, last4: &str) -> String {
    let brand = if brand.trim().is_empty() {
        "CARTÃO".to_string()
    } else {
        brand.to_uppercase()
    };
    format!("{brand} •••• {last4}")
}

/// Reais → centavos (o gateway trabalha em centavos inteiros).
fn to_cents(reais: f64) -> i64 {
    (reais * 100.0).round() as i64
}
