use std::sync::Arc;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use chrono::{Datelike, NaiveDate};
use uuid::Uuid;

use super::model::{
    Invoice, InvoiceStatus, PaymentMethod, PendingSummary, Plan, PlanKind, Subscription,
    SubscriptionStatus,
};
use super::repository::SubscriptionRepository;
use crate::error::CoreError;
use crate::util::add_months;

/// Service de Assinatura — orquestra catálogo + persistência.
///
/// Regras aplicadas (AI_RULES.md §1, §11, §14):
/// - Catálogo de planos: hardcoded em `available_plans` por enquanto.
///   Quando o painel super-admin existir, troca por consulta a tabela
///   `plans` sincronizada — a assinatura desta interface não muda.
/// - Validação centralizada (UI nunca confia em dados de entrada).
///
/// Janela de tolerância depois do vencimento até a assinatura virar
/// `Overdue`. Valor "pronto-pra-ajustar" — mantemos centralizado.
pub const OVERDUE_GRACE_DAYS: i64 = 7;

/// Intervalo do billing loop no server (em segundos). 1h é
/// suficiente para cobrar no início do dia em fuso BR. Reduzir para
/// testes manuais.
pub const BILLING_TICK_INTERVAL_SECS: u64 = 3600;

pub struct SubscriptionService {
    repo: Arc<dyn SubscriptionRepository>,
}

/// Termos efetivos de cobrança de uma assinatura (resolvidos via
/// [`SubscriptionService::terms`]): nome do plano, valor por ciclo e meses
/// por ciclo — do snapshot do catálogo ou do plano fixo (legado).
pub struct PlanTerms {
    pub name: String,
    /// Valor cobrado por ciclo, JÁ com o desconto abatido.
    pub amount: Decimal,
    pub months: u32,
    /// Valor por ciclo ANTES do desconto (preço de tabela do plano).
    pub gross_amount: Decimal,
    /// Desconto em R$ por mês concedido ao estabelecimento (0 = nenhum).
    pub discount_monthly: Decimal,
}

/// Valor cobrado por ciclo já com o desconto comercial abatido.
///
/// `gross` é o preço de tabela do ciclo inteiro; `discount_monthly` é o
/// desconto em R$/mês (nunca negativo) e `months` os meses do ciclo — o
/// desconto incide sobre cada mês. Resultado nunca abaixo de zero.
///
/// Pura/testável (§13) — a UI nunca calcula preço; o backend é a fonte.
pub fn charge_amount(gross: Decimal, discount_monthly: Decimal, months: u32) -> Decimal {
    (gross - discount_monthly.max(Decimal::ZERO) * Decimal::from(months)).max(Decimal::ZERO)
}

impl SubscriptionService {
    pub fn new(repo: Arc<dyn SubscriptionRepository>) -> Self {
        Self { repo }
    }

    /// Catálogo de planos — fonte da verdade até o super-admin existir.
    /// Mantemos a função no service (não como `const`) porque os
    /// labels em pt-BR e os cálculos de economia dependem do contexto.
    pub fn available_plans(&self) -> Vec<Plan> {
        let monthly = dec!(200);
        let semestral_monthly = dec!(190);
        let annual_monthly = dec!(180);
        vec![
            Plan {
                kind: PlanKind::Monthly,
                label: "Mensal".into(),
                monthly_price: monthly,
                total_per_charge: monthly,
                savings_label: String::new(),
                highlight_label: String::new(),
                description: "Cobrado todo mês · Cancele quando quiser".into(),
            },
            Plan {
                kind: PlanKind::Semestral,
                label: "Semestral".into(),
                monthly_price: semestral_monthly,
                total_per_charge: semestral_monthly * dec!(6),
                savings_label: format!(
                    "ECONOMIZE R$ {}/MÊS",
                    (monthly - semestral_monthly).trunc().to_i64().unwrap_or(0)
                ),
                highlight_label: String::new(),
                description: format!(
                    "Cobrado a cada 6 meses · R$ {}/mês",
                    semestral_monthly.trunc().to_i64().unwrap_or(0)
                ),
            },
            Plan {
                kind: PlanKind::Annual,
                label: "Anual".into(),
                monthly_price: annual_monthly,
                total_per_charge: annual_monthly * dec!(12),
                savings_label: format!(
                    "ECONOMIZE R$ {}/MÊS",
                    (monthly - annual_monthly).trunc().to_i64().unwrap_or(0)
                ),
                highlight_label: "MELHOR VALOR".into(),
                description: format!("Cobrado 1× por ano · R$ {}/mês", annual_monthly.trunc().to_i64().unwrap_or(0)),
            },
        ]
    }

    pub fn plan_for(&self, kind: PlanKind) -> Plan {
        self.available_plans()
            .into_iter()
            .find(|p| p.kind == kind)
            .expect("planos hardcoded sempre cobrem PlanKind")
    }

    /// Termos efetivos de cobrança da assinatura (nome, valor por ciclo,
    /// meses por ciclo). Usa o SNAPSHOT do plano do catálogo se houver;
    /// senão cai no plano fixo (`plan_for(plan_kind)`) — assinaturas legadas.
    /// É a fonte única do billing (Fase 2), para não depender do catálogo.
    pub fn terms(&self, sub: &Subscription) -> PlanTerms {
        let (name, gross, months) = if sub.is_catalog_plan() {
            (
                sub.plan_name.clone(),
                sub.plan_amount,
                sub.plan_period_months.max(1) as u32,
            )
        } else {
            let p = self.plan_for(sub.plan_kind);
            (p.label, p.total_per_charge, p.kind.months_per_charge())
        };
        // Desconto por mês abatido do ciclo inteiro; nunca abaixo de zero.
        let discount_monthly = sub.plan_discount_monthly.max(Decimal::ZERO);
        let amount = charge_amount(gross, discount_monthly, months);
        PlanTerms {
            name,
            amount,
            months,
            gross_amount: gross,
            discount_monthly,
        }
    }

    /// Define o desconto comercial (R$/mês) do estabelecimento. Preservado
    /// ao trocar de plano (subscribe_to_plan não toca neste campo).
    pub async fn set_plan_discount(
        &self,
        company_id: Uuid,
        discount_monthly: Decimal,
    ) -> Result<Subscription, CoreError> {
        let mut sub = self
            .repo
            .find_current(company_id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Assinatura não encontrada".into()))?;
        sub.plan_discount_monthly = discount_monthly.max(Decimal::ZERO);
        sub.base.updated_at = chrono::Utc::now().naive_utc();
        sub.base.synced = false;
        self.repo.update_subscription(&sub).await?;
        Ok(sub)
    }

    /// Assina um plano do CATÁLOGO (gerido pelo super admin): grava o
    /// snapshot dos termos na assinatura e aplica o período gratuito
    /// (trial) na 1ª cobrança. Recorrência ativa (cartão/PIX) deve ser
    /// cancelada antes, pois o valor muda (§ igual `change_plan`).
    pub async fn subscribe_to_plan(
        &self,
        company_id: Uuid,
        plan: &crate::plan::model::Plan,
        today: NaiveDate,
    ) -> Result<Subscription, CoreError> {
        let mut sub = self
            .repo
            .find_current(company_id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Assinatura não encontrada".into()))?;
        if sub.has_active_card() {
            return Err(CoreError::Validation(
                "Cancele o cartão recorrente antes de trocar de plano e cadastre-o novamente com o novo valor.".into(),
            ));
        }
        if sub.has_active_pix_auto() {
            return Err(CoreError::Validation(
                "Cancele o PIX Automático antes de trocar de plano e ative-o novamente com o novo valor.".into(),
            ));
        }
        sub.plan_id = Some(plan.id);
        sub.plan_name = plan.name.clone();
        sub.plan_amount = plan.amount;
        sub.plan_period_months = plan.period_months.max(1);
        sub.trial_days = plan.trial_days.max(0);
        sub.status = SubscriptionStatus::Active;
        // 1ª cobrança: após o trial (se houver) ou ao fim do 1º ciclo.
        sub.next_charge_date = Some(if plan.trial_days > 0 {
            today + chrono::Duration::days(plan.trial_days as i64)
        } else {
            add_months(today, plan.period_months.max(1))
        });
        sub.base.updated_at = chrono::Utc::now().naive_utc();
        sub.base.synced = false;
        self.repo.update_subscription(&sub).await?;
        Ok(sub)
    }

    pub async fn find_current(
        &self,
        company_id: Uuid,
    ) -> Result<Option<Subscription>, CoreError> {
        self.repo.find_current(company_id).await
    }

    pub async fn find_invoices(&self, company_id: Uuid) -> Result<Vec<Invoice>, CoreError> {
        self.repo.find_invoices(company_id).await
    }

    /// Garante uma assinatura ativa + faturas históricas na 1ª execução
    /// (defesa em profundidade — quando o super-admin estiver online,
    /// o pull traz os dados reais e o `sync_upsert` os sobrepõe).
    ///
    /// `today` é injetado para testabilidade.
    pub async fn ensure_seed(
        &self,
        company_id: Uuid,
        today: NaiveDate,
    ) -> Result<Subscription, CoreError> {
        if let Some(existing) = self.repo.find_current(company_id).await? {
            return Ok(existing);
        }
        let mut sub = Subscription::new(company_id, PlanKind::Monthly);
        sub.next_charge_date = Some(next_charge_after(today, PlanKind::Monthly));
        self.repo.create_subscription(&sub).await?;
        self.seed_history(company_id, &sub, today).await?;
        Ok(sub)
    }

    async fn seed_history(
        &self,
        company_id: Uuid,
        sub: &Subscription,
        today: NaiveDate,
    ) -> Result<(), CoreError> {
        let existing = self.repo.find_invoices(company_id).await?;
        if !existing.is_empty() {
            return Ok(());
        }
        let plan = self.plan_for(sub.plan_kind);
        // 5 cobranças mensais já pagas — alterna VISA/PIX como no mock.
        for i in 1..=5 {
            let issued = subtract_months(today, i);
            let number = format!("NFS-{:04}", 80 + (5 - i));
            let (method_kind, method_label) = if i % 2 == 0 {
                ("pix".to_string(), "PIX".to_string())
            } else {
                ("card".to_string(), "•••• 4242".to_string())
            };
            let paid_at = issued.and_hms_opt(12, 0, 0);
            let invoice = Invoice::new(
                company_id,
                sub.base.id,
                number,
                "Assinatura · Plano Mensal".into(),
                plan.monthly_price,
                method_kind,
                method_label,
                InvoiceStatus::Paid,
                issued,
                paid_at,
            );
            self.repo.create_invoice(&invoice).await?;
        }
        Ok(())
    }

    /// Troca de plano. Atualiza `plan_kind` + `next_charge_date`.
    ///
    /// Regras aplicadas (AI_RULES.md §11):
    /// - **Bloqueia** a troca quando há recorrência ativa (cartão ou Pix
    ///   Automático): o mandato é de valor fixo e mudar o plano mudaria
    ///   o valor cobrado. O cliente precisa cancelar a recorrência,
    ///   trocar de plano e reativar com o novo valor. Guarda no core
    ///   para valer offline + na API do server (UI nunca é confiada).
    pub async fn change_plan(
        &self,
        company_id: Uuid,
        plan: PlanKind,
        today: NaiveDate,
    ) -> Result<Subscription, CoreError> {
        let mut sub = self
            .repo
            .find_current(company_id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Assinatura não encontrada".into()))?;
        if sub.plan_kind == plan {
            return Ok(sub);
        }
        if sub.has_active_card() {
            return Err(CoreError::Validation(
                "Cancele o cartão recorrente antes de trocar de plano e cadastre-o novamente com o novo valor.".into(),
            ));
        }
        if sub.has_active_pix_auto() {
            return Err(CoreError::Validation(
                "Cancele o PIX Automático antes de trocar de plano e ative-o novamente com o novo valor.".into(),
            ));
        }
        sub.plan_kind = plan;
        sub.status = SubscriptionStatus::Active;
        sub.next_charge_date = Some(next_charge_after(today, plan));
        sub.base.updated_at = chrono::Utc::now().naive_utc();
        sub.base.synced = false;
        self.repo.update_subscription(&sub).await?;
        Ok(sub)
    }

    /// Marca uma fatura como paga. Chamado pelo handler PIX quando o
    /// polling confirma `Paid` no gateway. `paid_at` é o horário que o
    /// PSP devolveu; se vazio, usamos `now`.
    ///
    /// Regras aplicadas (AI_RULES.md §7, §11):
    /// - `synced = false` para que o SyncWorker leve a mudança ao server.
    /// - Idempotente: chamar de novo numa fatura já paga não altera nada.
    pub async fn mark_invoice_paid(
        &self,
        company_id: Uuid,
        invoice_id: Uuid,
        paid_at: Option<chrono::NaiveDateTime>,
    ) -> Result<Invoice, CoreError> {
        let all = self.repo.find_invoices(company_id).await?;
        let mut inv = all
            .into_iter()
            .find(|i| i.base.id == invoice_id)
            .ok_or_else(|| CoreError::NotFound("Fatura não encontrada".into()))?;
        if matches!(inv.status, InvoiceStatus::Paid) {
            return Ok(inv);
        }
        inv.status = InvoiceStatus::Paid;
        inv.paid_at = paid_at.or_else(|| Some(chrono::Utc::now().naive_utc()));
        inv.base.updated_at = chrono::Utc::now().naive_utc();
        inv.base.synced = false;
        self.repo.update_invoice(&inv).await?;

        // Auto-recover: se a assinatura estava `Overdue` e não restam
        // mais faturas Pending, volta para `Active` automaticamente.
        // Idempotente — `mark_active` retorna cedo se já estiver Active.
        self.recover_status_if_settled(company_id).await?;

        Ok(inv)
    }

    /// Após qualquer mudança em fatura, verifica se a assinatura
    /// pode voltar para `Active` (saiu da inadimplência). Não falha
    /// se a assinatura não existir — operação best-effort.
    async fn recover_status_if_settled(&self, company_id: Uuid) -> Result<(), CoreError> {
        let Some(sub) = self.repo.find_current(company_id).await? else {
            return Ok(());
        };
        if !matches!(sub.status, SubscriptionStatus::Overdue) {
            return Ok(());
        }
        let still_pending = self
            .repo
            .find_invoices(company_id)
            .await?
            .into_iter()
            .any(|i| matches!(i.status, InvoiceStatus::Pending));
        if still_pending {
            return Ok(());
        }
        self.mark_active(sub.base.id).await?;
        Ok(())
    }

    /// Atualiza forma de pagamento. Quando o gateway entrar, validar
    /// token + autorização aqui dentro.
    pub async fn update_payment_method(
        &self,
        company_id: Uuid,
        method: PaymentMethod,
    ) -> Result<Subscription, CoreError> {
        let mut sub = self
            .repo
            .find_current(company_id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Assinatura não encontrada".into()))?;
        sub.payment_method = method;
        sub.base.updated_at = chrono::Utc::now().naive_utc();
        sub.base.synced = false;
        self.repo.update_subscription(&sub).await?;
        Ok(sub)
    }

    // ── Cartão recorrente (gateway de assinaturas) ──────────────

    /// Vincula um cartão recorrente já criado no gateway. Grava o
    /// `gateway_subscription_id` (para cancelar/reconciliar) e troca a
    /// forma de pagamento embutida para o cartão. A partir daqui o
    /// billing loop ignora esta assinatura (o gateway dirige a
    /// recorrência — ver [`Subscription::has_active_card`]).
    ///
    /// Regras aplicadas (AI_RULES.md §7, §11): `synced = false` para o
    /// SyncWorker propagar ao desktop.
    pub async fn bind_card(
        &self,
        company_id: Uuid,
        gateway: String,
        gateway_subscription_id: String,
        card_label: String,
        card_expiry: String,
        card_status: String,
        next_charge_date: Option<NaiveDate>,
    ) -> Result<Subscription, CoreError> {
        let mut sub = self
            .repo
            .find_current(company_id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Assinatura não encontrada".into()))?;
        sub.payment_method = PaymentMethod {
            kind: "card".into(),
            label: card_label,
            expiry: card_expiry,
        };
        sub.gateway = Some(gateway);
        sub.gateway_subscription_id = Some(gateway_subscription_id);
        sub.card_status = Some(card_status);
        sub.status = SubscriptionStatus::Active;
        if let Some(d) = next_charge_date {
            sub.next_charge_date = Some(d);
        }
        sub.base.updated_at = chrono::Utc::now().naive_utc();
        sub.base.synced = false;
        self.repo.update_subscription(&sub).await?;
        Ok(sub)
    }

    /// Desvincula o cartão (após cancelar no gateway). Volta a forma de
    /// pagamento para PIX manual e limpa os campos do gateway. O
    /// `next_charge_date` é preservado — o billing loop volta a dirigir
    /// a recorrência via PIX.
    pub async fn cancel_card(&self, company_id: Uuid) -> Result<Subscription, CoreError> {
        let mut sub = self
            .repo
            .find_current(company_id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Assinatura não encontrada".into()))?;
        sub.payment_method = PaymentMethod {
            kind: "pix".into(),
            label: "PIX Automático".into(),
            expiry: String::new(),
        };
        sub.gateway = None;
        sub.gateway_subscription_id = None;
        sub.card_status = Some("canceled".into());
        sub.base.updated_at = chrono::Utc::now().naive_utc();
        sub.base.synced = false;
        self.repo.update_subscription(&sub).await?;
        Ok(sub)
    }

    pub async fn find_by_gateway_subscription_id(
        &self,
        gateway_subscription_id: &str,
    ) -> Result<Option<Subscription>, CoreError> {
        self.repo
            .find_by_gateway_subscription_id(gateway_subscription_id)
            .await
    }

    /// Reconcilia uma cobrança de cartão recebida via webhook. Garante
    /// uma invoice no ciclo corrente e a marca paga/falha conforme o
    /// status do gateway, ajustando o estado da assinatura.
    ///
    /// Idempotente: reprocessar a mesma notificação não duplica invoice
    /// (reusa a do mês) nem altera uma já paga.
    pub async fn apply_card_charge(
        &self,
        sub: &Subscription,
        charge_status: &str,
        amount: Decimal,
        paid_at: Option<chrono::NaiveDateTime>,
        today: NaiveDate,
    ) -> Result<(), CoreError> {
        self.apply_recurring_charge(sub, charge_status, amount, paid_at, today, "cartão")
            .await
    }

    /// Reconcilia uma cobrança recorrente (cartão ou Pix Automático)
    /// recebida via webhook. Garante uma invoice no ciclo corrente e a
    /// marca paga/falha conforme o status do gateway. Idempotente.
    async fn apply_recurring_charge(
        &self,
        sub: &Subscription,
        charge_status: &str,
        amount: Decimal,
        paid_at: Option<chrono::NaiveDateTime>,
        today: NaiveDate,
        method_note: &str,
    ) -> Result<(), CoreError> {
        let company_id = sub.base.company_id;
        // 1) Garante uma invoice no ciclo corrente (reusa se já existe).
        let invoice = match self
            .repo
            .find_invoice_in_month(sub.base.id, today.year(), today.month())
            .await?
        {
            Some(inv) => inv,
            None => {
                let terms = self.terms(sub);
                let number =
                    generate_invoice_number(today, &self.repo, company_id).await?;
                // "Consta" o desconto na descrição da fatura quando houver.
                let desc = if terms.discount_monthly > Decimal::ZERO {
                    format!(
                        "Assinatura · Plano {} ({method_note}) · desconto R$ {}/mês",
                        terms.name, crate::money::round2(terms.discount_monthly)
                    )
                } else {
                    format!("Assinatura · Plano {} ({method_note})", terms.name)
                };
                let new = Invoice::new(
                    company_id,
                    sub.base.id,
                    number,
                    desc,
                    amount,
                    sub.payment_method.kind.clone(),
                    sub.payment_method.label.clone(),
                    InvoiceStatus::Pending,
                    today,
                    None,
                );
                self.repo.create_invoice(&new).await?;
                new
            }
        };
        // 2) Aplica o desfecho da cobrança.
        if is_paid_status(charge_status) {
            self.mark_invoice_paid(company_id, invoice.base.id, paid_at)
                .await?;
            // Avança o próximo ciclo (o gateway também cobra sozinho;
            // aqui é só para o "próxima cobrança" refletir).
            self.advance_next_charge(company_id, today).await?;
        } else if is_failed_status(charge_status) {
            self.mark_invoice_failed(company_id, invoice.base.id).await?;
            if let Some(s) = self.repo.find_current(company_id).await? {
                self.mark_overdue(s.base.id).await?;
            }
        }
        Ok(())
    }

    // ── Pix Automático (mandato de débito recorrente) ───────────

    /// Vincula um mandato de Pix Automático recém-criado (ainda
    /// pendente de autorização do pagador). Troca a forma de pagamento
    /// embutida para "PIX Automático".
    pub async fn bind_pix_auto(
        &self,
        company_id: Uuid,
        gateway: String,
        rec_id: String,
        status: String,
    ) -> Result<Subscription, CoreError> {
        let mut sub = self
            .repo
            .find_current(company_id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Assinatura não encontrada".into()))?;
        sub.payment_method = PaymentMethod {
            kind: "pix".into(),
            label: "PIX Automático".into(),
            expiry: String::new(),
        };
        sub.gateway = Some(gateway);
        sub.pix_auto_rec_id = Some(rec_id);
        sub.pix_auto_status = Some(status);
        sub.base.updated_at = chrono::Utc::now().naive_utc();
        sub.base.synced = false;
        self.repo.update_subscription(&sub).await?;
        Ok(sub)
    }

    /// Atualiza o status do mandato (após polling/webhook). Quando passa
    /// a ativo, garante `status = Active` e mantém `next_charge_date`.
    pub async fn set_pix_auto_status(
        &self,
        company_id: Uuid,
        status: String,
        next_charge_date: Option<NaiveDate>,
    ) -> Result<Subscription, CoreError> {
        let mut sub = self
            .repo
            .find_current(company_id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Assinatura não encontrada".into()))?;
        sub.pix_auto_status = Some(status);
        if let Some(d) = next_charge_date {
            sub.next_charge_date = Some(d);
        }
        if sub.has_active_pix_auto() {
            sub.status = SubscriptionStatus::Active;
        }
        sub.base.updated_at = chrono::Utc::now().naive_utc();
        sub.base.synced = false;
        self.repo.update_subscription(&sub).await?;
        Ok(sub)
    }

    /// Desvincula o Pix Automático (após cancelar no gateway). Volta a
    /// forma de pagamento para PIX manual.
    pub async fn cancel_pix_auto(&self, company_id: Uuid) -> Result<Subscription, CoreError> {
        let mut sub = self
            .repo
            .find_current(company_id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Assinatura não encontrada".into()))?;
        sub.payment_method = PaymentMethod {
            kind: "pix".into(),
            label: "PIX manual".into(),
            expiry: String::new(),
        };
        sub.pix_auto_rec_id = None;
        sub.pix_auto_status = Some("canceled".into());
        sub.gateway = None;
        sub.base.updated_at = chrono::Utc::now().naive_utc();
        sub.base.synced = false;
        self.repo.update_subscription(&sub).await?;
        Ok(sub)
    }

    pub async fn find_by_pix_auto_rec_id(
        &self,
        rec_id: &str,
    ) -> Result<Option<Subscription>, CoreError> {
        self.repo.find_by_pix_auto_rec_id(rec_id).await
    }

    /// Reconcilia uma cobrança de Pix Automático recebida via webhook.
    pub async fn apply_pix_auto_charge(
        &self,
        sub: &Subscription,
        charge_status: &str,
        amount: Decimal,
        paid_at: Option<chrono::NaiveDateTime>,
        today: NaiveDate,
    ) -> Result<(), CoreError> {
        self.apply_recurring_charge(sub, charge_status, amount, paid_at, today, "PIX automático")
            .await
    }

    /// Marca uma invoice como `Failed`. Idempotente.
    async fn mark_invoice_failed(
        &self,
        company_id: Uuid,
        invoice_id: Uuid,
    ) -> Result<(), CoreError> {
        let mut inv = self
            .repo
            .find_invoices(company_id)
            .await?
            .into_iter()
            .find(|i| i.base.id == invoice_id)
            .ok_or_else(|| CoreError::NotFound("Fatura não encontrada".into()))?;
        if matches!(inv.status, InvoiceStatus::Failed) {
            return Ok(());
        }
        inv.status = InvoiceStatus::Failed;
        inv.base.updated_at = chrono::Utc::now().naive_utc();
        inv.base.synced = false;
        self.repo.update_invoice(&inv).await
    }

    /// Reposiciona `next_charge_date` para o próximo ciclo a partir de
    /// hoje. Usado quando o gateway confirma uma cobrança de cartão ou
    /// quando emitimos um `cobr` de Pix Automático.
    pub async fn advance_next_charge(
        &self,
        company_id: Uuid,
        today: NaiveDate,
    ) -> Result<(), CoreError> {
        let Some(mut sub) = self.repo.find_current(company_id).await? else {
            return Ok(());
        };
        sub.next_charge_date = Some(next_charge_after(today, sub.plan_kind));
        sub.base.updated_at = chrono::Utc::now().naive_utc();
        sub.base.synced = false;
        self.repo.update_subscription(&sub).await
    }

    // ── Visibilidade de cobrança (Fase 14C) ─────────────────────

    /// Resumo de pendências para a UI: nº de faturas em aberto + se a
    /// assinatura está em atraso. Usado pela sidebar (badge) e pelo
    /// toast inicial. Não roda nenhuma chamada de gateway.
    pub async fn pending_summary(
        &self,
        company_id: Uuid,
        today: NaiveDate,
    ) -> Result<PendingSummary, CoreError> {
        let sub = self.repo.find_current(company_id).await?;
        let invoices = self.repo.find_invoices(company_id).await?;
        let pending: Vec<&Invoice> = invoices
            .iter()
            .filter(|i| matches!(i.status, InvoiceStatus::Pending))
            .collect();
        let is_overdue = sub
            .as_ref()
            .map(|s| matches!(s.status, SubscriptionStatus::Overdue))
            .unwrap_or(false);
        let next_charge_date = sub.as_ref().and_then(|s| s.next_charge_date);
        let days_until_next_charge =
            next_charge_date.map(|d| (d - today).num_days());
        Ok(PendingSummary {
            pending_invoice_count: pending.len() as u32,
            // Considera "ação necessária" qualquer fatura pending OU status overdue.
            // Quando ambos zerados, badge na sidebar some.
            action_count: pending.len() as u32 + if is_overdue { 1 } else { 0 },
            is_overdue,
            next_charge_date,
            days_until_next_charge,
        })
    }

    // ── Billing loop (cobrança recorrente) ──────────────────────

    /// Assinaturas com `next_charge_date <= today` e `status = Active`.
    /// Loop server itera sobre essas e dispara cobrança PIX.
    pub async fn find_due_subscriptions(
        &self,
        today: chrono::NaiveDate,
    ) -> Result<Vec<Subscription>, CoreError> {
        self.repo.find_due_subscriptions(today).await
    }

    /// Cria a invoice da próxima cobrança e atualiza `next_charge_date`
    /// para o próximo ciclo. Idempotente — se já houver uma invoice
    /// emitida no mês corrente, devolve a existente sem criar nova.
    ///
    /// Regras aplicadas (AI_RULES.md §7, §11):
    /// - Toda escrita marca `synced = false` (sync leva ao desktop).
    /// - Sem cobrança real — esse passo é responsabilidade do server
    ///   chamar o gateway com `invoice.id` + `invoice.amount`.
    pub async fn record_charge_attempt(
        &self,
        subscription_id: Uuid,
        today: chrono::NaiveDate,
    ) -> Result<Invoice, CoreError> {
        let mut sub = self.find_subscription_by_id(subscription_id).await?;
        let terms = self.terms(&sub);
        // Idempotência: já existe invoice nesse mês?
        if let Some(existing) = self
            .repo
            .find_invoice_in_month(subscription_id, today.year(), today.month())
            .await?
        {
            return Ok(existing);
        }
        let number = generate_invoice_number(today, &self.repo, sub.base.company_id).await?;
        let description = format!(
            "Assinatura · Plano {} ({}–{})",
            terms.name,
            today.format("%m/%Y"),
            chrono::NaiveDate::from_ymd_opt(today.year(), today.month(), 1)
                .map(|d| add_months(d, terms.months as i32 - 1))
                .map(|d| d.format("%m/%Y").to_string())
                .unwrap_or_default()
        );
        let invoice = Invoice::new(
            sub.base.company_id,
            subscription_id,
            number,
            description,
            terms.amount,
            sub.payment_method.kind.clone(),
            sub.payment_method.label.clone(),
            InvoiceStatus::Pending,
            today,
            None,
        );
        self.repo.create_invoice(&invoice).await?;
        // Atualiza next_charge_date para o próximo ciclo (período do plano).
        sub.next_charge_date = Some(add_months(today, terms.months as i32));
        sub.base.updated_at = chrono::Utc::now().naive_utc();
        sub.base.synced = false;
        self.repo.update_subscription(&sub).await?;
        Ok(invoice)
    }

    /// Marca a assinatura como `Overdue`. Idempotente.
    pub async fn mark_overdue(&self, subscription_id: Uuid) -> Result<Subscription, CoreError> {
        let mut sub = self.find_subscription_by_id(subscription_id).await?;
        if matches!(sub.status, SubscriptionStatus::Overdue) {
            return Ok(sub);
        }
        sub.status = SubscriptionStatus::Overdue;
        sub.base.updated_at = chrono::Utc::now().naive_utc();
        sub.base.synced = false;
        self.repo.update_subscription(&sub).await?;
        Ok(sub)
    }

    /// Marca a assinatura como `Active` novamente (após confirmação
    /// de pagamento, por exemplo).
    pub async fn mark_active(&self, subscription_id: Uuid) -> Result<Subscription, CoreError> {
        let mut sub = self.find_subscription_by_id(subscription_id).await?;
        if matches!(sub.status, SubscriptionStatus::Active) {
            return Ok(sub);
        }
        sub.status = SubscriptionStatus::Active;
        sub.base.updated_at = chrono::Utc::now().naive_utc();
        sub.base.synced = false;
        self.repo.update_subscription(&sub).await?;
        Ok(sub)
    }

    /// Lista assinaturas candidatas a `Overdue` (invoice em aberto há
    /// > `OVERDUE_GRACE_DAYS` dias).
    pub async fn find_overdue_candidates(
        &self,
        today: chrono::NaiveDate,
    ) -> Result<Vec<Subscription>, CoreError> {
        self.repo
            .find_overdue_candidates(today, OVERDUE_GRACE_DAYS)
            .await
    }

    async fn find_subscription_by_id(
        &self,
        subscription_id: Uuid,
    ) -> Result<Subscription, CoreError> {
        // Busca direta pelo id no repositório — independe de status ou
        // `next_charge_date` (antes varria só due/overdue, falhando para
        // assinaturas ativas com cobrança futura).
        self.repo
            .find_subscription_by_id(subscription_id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Assinatura não encontrada".into()))
    }

    // ── Sync (§7) ───────────────────────────────────────────────

    pub async fn find_unsynced_subscriptions(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<Subscription>, CoreError> {
        self.repo.find_unsynced_subscriptions(company_id).await
    }

    pub async fn find_unsynced_invoices(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<Invoice>, CoreError> {
        self.repo.find_unsynced_invoices(company_id).await
    }

    pub async fn mark_subscription_synced(
        &self,
        company_id: Uuid,
        id: Uuid,
        updated_at: chrono::NaiveDateTime,
    ) -> Result<(), CoreError> {
        self.repo.mark_subscription_synced(company_id, id, updated_at).await
    }

    pub async fn mark_invoice_synced(
        &self,
        company_id: Uuid,
        id: Uuid,
        updated_at: chrono::NaiveDateTime,
    ) -> Result<(), CoreError> {
        self.repo.mark_invoice_synced(company_id, id, updated_at).await
    }

    pub async fn sync_upsert_subscription(
        &self,
        company_id: Uuid,
        mut s: Subscription,
    ) -> Result<(), CoreError> {
        if s.base.company_id != company_id {
            return Err(CoreError::Validation("Company mismatch".into()));
        }
        s.base.synced = true;
        self.repo.sync_upsert_subscription(&s).await
    }

    pub async fn sync_upsert_invoice(
        &self,
        company_id: Uuid,
        mut inv: Invoice,
    ) -> Result<(), CoreError> {
        if inv.base.company_id != company_id {
            return Err(CoreError::Validation("Company mismatch".into()));
        }
        inv.base.synced = true;
        self.repo.sync_upsert_invoice(&inv).await
    }

    pub async fn find_subscriptions_updated_since(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
    ) -> Result<Vec<Subscription>, CoreError> {
        self.repo
            .find_subscriptions_updated_since(company_id, since)
            .await
    }

    pub async fn find_invoices_updated_since(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
    ) -> Result<Vec<Invoice>, CoreError> {
        self.repo
            .find_invoices_updated_since(company_id, since)
            .await
    }
}

/// Status de cobrança do gateway que contam como **pagamento aprovado**.
/// Normalizados em minúsculas pela implementação concreta.
fn is_paid_status(s: &str) -> bool {
    matches!(s, "paid" | "settled" | "approved" | "active")
}

/// Status que contam como **falha/recusa** (gera atraso).
fn is_failed_status(s: &str) -> bool {
    matches!(
        s,
        "unpaid" | "refused" | "declined" | "canceled" | "cancelled" | "expired" | "contested"
    )
}

/// Próxima data de cobrança ≅ hoje + (meses do plano).
fn next_charge_after(today: NaiveDate, kind: PlanKind) -> NaiveDate {
    add_months(today, kind.months_per_charge() as i32)
}

fn subtract_months(date: NaiveDate, months: i32) -> NaiveDate {
    add_months(date, -months)
}

/// Gera um número de fatura sequencial baseado na contagem atual de
/// invoices da empresa. Não é "atômico" (race condition possível em
/// concorrência alta) mas o billing loop roda single-thread por
/// company — suficiente para esta fase.
async fn generate_invoice_number(
    today: NaiveDate,
    repo: &Arc<dyn SubscriptionRepository>,
    company_id: Uuid,
) -> Result<String, CoreError> {
    let existing = repo.find_invoices(company_id).await?;
    let next = existing.len() as u32 + 81;
    Ok(format!("NFS-{:04}-{}", next, today.format("%Y")))
}
