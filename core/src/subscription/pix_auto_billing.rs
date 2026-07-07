use std::sync::Arc;

use chrono::NaiveDate;
use uuid::Uuid;

use super::model::Subscription;
use super::service::SubscriptionService;
use crate::error::CoreError;
use crate::payment_gateway::pix_auto::{
    is_active_status, is_rejected_status, CreatedRecurrence, PixAutoCustomer, PixAutoGateway,
    PixAutoInput,
};

/// Orquestra o Pix Automático: cria o mandato (recorrência), reflete o
/// estado na assinatura e gera as cobranças recorrentes (`cobr`).
///
/// Regras aplicadas (AI_RULES.md §1, §8, §11):
/// - Lógica de negócio aqui; rotas só convertem HTTP.
/// - O gateway concreto (Efi) entra por trait — core não conhece HTTP.
/// - Valor **fixo** por ciclo; **cobra a 1ª já** ao autorizar (decisões
///   do projeto).
pub struct PixAutoBillingService {
    gateway: Arc<dyn PixAutoGateway>,
    subscriptions: Arc<SubscriptionService>,
    notification_url: String,
}

impl PixAutoBillingService {
    pub fn new(
        gateway: Arc<dyn PixAutoGateway>,
        subscriptions: Arc<SubscriptionService>,
        notification_url: String,
    ) -> Self {
        Self {
            gateway,
            subscriptions,
            notification_url,
        }
    }

    /// Cria o mandato de Pix Automático e devolve o QR de **autorização**
    /// para o pagador aprovar no app do banco dele. A assinatura fica
    /// com `pix_auto_status = pending` até a autorização.
    pub async fn activate(
        &self,
        company_id: Uuid,
        customer_name: String,
        customer_cpf: String,
    ) -> Result<(Subscription, CreatedRecurrence), CoreError> {
        if customer_cpf.chars().filter(|c| c.is_ascii_digit()).count() < 11 {
            return Err(CoreError::Validation("CPF/CNPJ inválido".into()));
        }
        let sub = self
            .subscriptions
            .find_current(company_id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Assinatura não encontrada".into()))?;
        let terms = self.subscriptions.terms(&sub);
        let input = PixAutoInput {
            amount_cents: crate::money::to_cents(terms.amount),
            interval_months: terms.months,
            plan_name: format!("LETAF · Plano {}", terms.name),
            description: format!("Assinatura LETAF · {}", terms.name),
            customer: PixAutoCustomer {
                name: customer_name,
                cpf: customer_cpf,
            },
            notification_url: self.notification_url.clone(),
            custom_id: sub.base.id.to_string(),
        };
        let created = self.gateway.create_recurrence(&input).await?;
        let updated = self
            .subscriptions
            .bind_pix_auto(
                company_id,
                self.gateway.name().to_string(),
                created.rec_id.clone(),
                created.status.clone(),
            )
            .await?;
        Ok((updated, created))
    }

    /// Consulta o status do mandato (polling da autorização). Na
    /// transição para **ativo**, emite a 1ª cobrança (`cobr`) e avança
    /// o ciclo (decisão "cobra a 1ª já").
    pub async fn refresh(
        &self,
        company_id: Uuid,
        today: NaiveDate,
    ) -> Result<Subscription, CoreError> {
        let sub = self
            .subscriptions
            .find_current(company_id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Assinatura não encontrada".into()))?;
        let Some(rec_id) = sub.pix_auto_rec_id.clone() else {
            return Ok(sub);
        };
        let was_active = sub.has_active_pix_auto();
        let status = self.gateway.fetch_recurrence_status(&rec_id).await?;
        let updated = self
            .subscriptions
            .set_pix_auto_status(company_id, status.status.clone(), status.next_charge_date)
            .await?;
        // Primeira ativação (jornada 3): a 1ª cobrança foi o cob imediato,
        // pago ao autorizar. Registramos como paga (cria a fatura +
        // avança o ciclo). O billing loop emite `cobr` a partir do próximo.
        if !was_active && is_active_status(&status.status) {
            let terms = self.subscriptions.terms(&updated);
            self.subscriptions
                .apply_pix_auto_charge(&updated, "paid", terms.amount, None, today)
                .await?;
        } else if is_rejected_status(&status.status) {
            // Mandato recusado/encerrado pelo banco do pagador: o débito
            // automático não vai mais ocorrer. Desvincula o Pix Automático
            // para o billing loop parar de emitir `cobr` num `rec` morto e
            // cair no PIX manual. Não é punitivo (não marca Overdue).
            return self.subscriptions.cancel_pix_auto(company_id).await;
        }
        Ok(updated)
    }

    /// Emite a cobrança recorrente (`cobr`) de um ciclo e avança o
    /// `next_charge_date`. Chamado na 1ª ativação e pelo billing loop.
    pub async fn charge_cycle(
        &self,
        sub: &Subscription,
        today: NaiveDate,
    ) -> Result<(), CoreError> {
        let Some(rec_id) = sub.pix_auto_rec_id.as_deref() else {
            return Ok(());
        };
        let terms = self.subscriptions.terms(sub);
        let amount_cents = crate::money::to_cents(terms.amount);
        let description = format!("Assinatura LETAF · {}", terms.name);
        self.gateway
            .create_recurring_charge(
                rec_id,
                amount_cents,
                today,
                &description,
                &sub.base.id.to_string(),
            )
            .await?;
        // Avança o ciclo já na emissão (o webhook só confirma o débito).
        self.subscriptions
            .advance_next_charge(sub.base.company_id, today)
            .await?;
        Ok(())
    }

    /// Cancela o mandato no gateway e desvincula localmente.
    pub async fn cancel(&self, company_id: Uuid) -> Result<Subscription, CoreError> {
        let sub = self
            .subscriptions
            .find_current(company_id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Assinatura não encontrada".into()))?;
        if let Some(rec_id) = sub.pix_auto_rec_id.as_deref() {
            self.gateway.cancel_recurrence(rec_id).await?;
        }
        self.subscriptions.cancel_pix_auto(company_id).await
    }

    /// Processa o corpo do webhook PIX: reconcilia cada débito (`cobr`)
    /// com a assinatura correspondente (por `idRec`).
    pub async fn apply_webhook(
        &self,
        body: &str,
        today: NaiveDate,
    ) -> Result<u32, CoreError> {
        let events = self.gateway.parse_webhook(body)?;
        let mut applied = 0;
        for ev in events {
            let Some(sub) = self
                .subscriptions
                .find_by_pix_auto_rec_id(&ev.rec_id)
                .await?
            else {
                continue;
            };
            self.subscriptions
                .apply_pix_auto_charge(&sub, &ev.status, ev.amount, ev.paid_at, today)
                .await?;
            applied += 1;
        }
        Ok(applied)
    }
}
