use chrono::{NaiveDate, NaiveDateTime};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::BaseFields;

/// Granularidade de cobrança. Valor canônico vai no banco como string;
/// `as_str` / `from_str` blindam contra typos.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlanKind {
    Monthly,
    Semestral,
    Annual,
}

impl PlanKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Monthly => "monthly",
            Self::Semestral => "semestral",
            Self::Annual => "annual",
        }
    }

    // `from_str` infalível (default em valor desconhecido); não é o
    // `FromStr` da std, que retorna `Result` — silenciamos o lint.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "semestral" => Self::Semestral,
            "annual" => Self::Annual,
            _ => Self::Monthly,
        }
    }

    /// Quantos meses cabem em uma cobrança deste plano.
    pub fn months_per_charge(self) -> u32 {
        match self {
            Self::Monthly => 1,
            Self::Semestral => 6,
            Self::Annual => 12,
        }
    }
}

/// Status da assinatura. Reservamos `Overdue` para inadimplência
/// futura — hoje o sistema só lida com `Active`/`Cancelled`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubscriptionStatus {
    Active,
    Cancelled,
    Overdue,
}

impl SubscriptionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Cancelled => "cancelled",
            Self::Overdue => "overdue",
        }
    }

    // `from_str` infalível (default em valor desconhecido); não é o
    // `FromStr` da std, que retorna `Result` — silenciamos o lint.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "cancelled" => Self::Cancelled,
            "overdue" => Self::Overdue,
            _ => Self::Active,
        }
    }
}

/// Forma de pagamento — embutida na assinatura porque por enquanto
/// é 1↔1 (uma assinatura tem uma forma de pagamento). Quando
/// permitirmos múltiplas, sai daqui.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentMethod {
    /// `card` ou `pix`.
    pub kind: String,
    /// Rótulo exibível ("•••• 4242", "PIX").
    pub label: String,
    /// Validade do cartão ("08/28"). Vazia para PIX.
    pub expiry: String,
}

impl PaymentMethod {
    pub fn placeholder_card() -> Self {
        Self {
            kind: "card".into(),
            label: "•••• 4242".into(),
            expiry: "08/28".into(),
        }
    }
}

/// Assinatura corrente da empresa (1 registro por company).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    #[serde(flatten)]
    pub base: BaseFields,
    pub plan_kind: PlanKind,
    pub next_charge_date: Option<NaiveDate>,
    pub status: SubscriptionStatus,
    pub payment_method: PaymentMethod,
    /// Gateway que mantém o cartão recorrente ("efi"). `None` quando a
    /// cobrança não está vinculada a cartão (ex.: PIX manual).
    #[serde(default)]
    pub gateway: Option<String>,
    /// ID da assinatura no gateway — usado para cancelar e reconciliar
    /// notificações. `None` enquanto não há cartão vinculado.
    #[serde(default)]
    pub gateway_subscription_id: Option<String>,
    /// Status da assinatura **no gateway** ("active"/"unpaid"/"canceled").
    /// Distinto de `status` (nosso estado interno).
    #[serde(default)]
    pub card_status: Option<String>,
    /// ID da recorrência de **Pix Automático** (`idRec`) no gateway.
    /// `None` quando não há mandato de Pix Automático.
    #[serde(default)]
    pub pix_auto_rec_id: Option<String>,
    /// Status do mandato de Pix Automático ("pending"/"active"/"canceled").
    #[serde(default)]
    pub pix_auto_status: Option<String>,

    // ── Plano do catálogo (Fase 2) — snapshot dos termos ──────────────
    // Quando a loja assina um plano do catálogo (super admin), guardamos
    // aqui os termos NO MOMENTO da assinatura, para o billing não depender
    // do catálogo (que pode mudar) nem do `plan_kind` fixo. `plan_id = None`
    // → assinatura legada (billing cai no `plan_for(plan_kind)`).
    #[serde(default)]
    pub plan_id: Option<Uuid>,
    #[serde(default)]
    pub plan_name: String,
    /// Valor cobrado por ciclo (R$) do plano do catálogo.
    #[serde(default)]
    pub plan_amount: Decimal,
    /// Meses por cobrança do plano do catálogo.
    #[serde(default)]
    pub plan_period_months: i32,
    /// Período gratuito (dias) do plano do catálogo (aplicado ao assinar).
    #[serde(default)]
    pub trial_days: i32,
    /// Desconto comercial em R$ POR MÊS concedido a este estabelecimento
    /// (definido pelo super admin). Abatido do valor cobrado por ciclo
    /// (`× meses`). `0` = sem desconto. Preservado ao trocar de plano.
    #[serde(default)]
    pub plan_discount_monthly: Decimal,
}

impl Subscription {
    pub fn new(company_id: Uuid, plan_kind: PlanKind) -> Self {
        Self {
            base: BaseFields::new(company_id),
            plan_kind,
            next_charge_date: None,
            status: SubscriptionStatus::Active,
            payment_method: PaymentMethod::placeholder_card(),
            gateway: None,
            gateway_subscription_id: None,
            card_status: None,
            pix_auto_rec_id: None,
            pix_auto_status: None,
            plan_id: None,
            plan_name: String::new(),
            plan_amount: Decimal::ZERO,
            plan_period_months: 0,
            trial_days: 0,
            plan_discount_monthly: Decimal::ZERO,
        }
    }

    /// `true` se a assinatura está num plano do catálogo (snapshot ativo).
    pub fn is_catalog_plan(&self) -> bool {
        self.plan_id.is_some()
    }

    /// `true` quando há um cartão recorrente ativo no gateway. O billing
    /// loop usa isto para NÃO cobrar PIX dessas assinaturas (o gateway
    /// dirige a recorrência do cartão).
    pub fn has_active_card(&self) -> bool {
        self.gateway_subscription_id.is_some()
            && self.payment_method.kind == "card"
    }

    /// `true` quando há um mandato de Pix Automático **autorizado**. O
    /// billing loop gera um `cobr` por ciclo (em vez do PIX manual) e o
    /// banco do pagador debita sozinho.
    pub fn has_active_pix_auto(&self) -> bool {
        self.pix_auto_rec_id.is_some()
            && self
                .pix_auto_status
                .as_deref()
                .map(crate::payment_gateway::pix_auto::is_active_status)
                .unwrap_or(false)
    }
}

/// Status da fatura.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InvoiceStatus {
    Pending,
    Paid,
    Failed,
}

impl InvoiceStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Paid => "paid",
            Self::Failed => "failed",
        }
    }

    // `from_str` infalível (default em valor desconhecido); não é o
    // `FromStr` da std, que retorna `Result` — silenciamos o lint.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "paid" => Self::Paid,
            "failed" => Self::Failed,
            _ => Self::Pending,
        }
    }
}

/// Fatura/Recibo do histórico de cobrança.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invoice {
    #[serde(flatten)]
    pub base: BaseFields,
    pub subscription_id: Uuid,
    pub number: String,
    pub description: String,
    pub amount: Decimal,
    pub method_kind: String,
    pub method_label: String,
    pub status: InvoiceStatus,
    pub issued_at: NaiveDate,
    pub paid_at: Option<NaiveDateTime>,
}

impl Invoice {
    pub fn new(
        company_id: Uuid,
        subscription_id: Uuid,
        number: String,
        description: String,
        amount: Decimal,
        method_kind: String,
        method_label: String,
        status: InvoiceStatus,
        issued_at: NaiveDate,
        paid_at: Option<NaiveDateTime>,
    ) -> Self {
        Self {
            base: BaseFields::new(company_id),
            subscription_id,
            number,
            description,
            amount,
            method_kind,
            method_label,
            status,
            issued_at,
            paid_at,
        }
    }
}

/// Resumo de pendências da assinatura para a UI.
///
/// Calculado em runtime pelo `SubscriptionService::pending_summary`.
/// Não persiste — é só um derivado pronto para consumo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingSummary {
    /// Faturas em status `Pending` — aguardando pagamento.
    pub pending_invoice_count: u32,
    /// Total para badge: `pending_invoice_count` + 1 se overdue.
    pub action_count: u32,
    /// Assinatura em atraso (status `Overdue`).
    pub is_overdue: bool,
    /// Próxima cobrança agendada (referência do card preto).
    pub next_charge_date: Option<chrono::NaiveDate>,
    /// Dias até a próxima cobrança. Negativo = já venceu.
    /// `None` quando `next_charge_date` é `None`.
    pub days_until_next_charge: Option<i64>,
}

/// Plano comercial — catálogo retornado pelo service. Não persiste
/// (por enquanto é constante no service).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub kind: PlanKind,
    /// "Mensal" / "Semestral" / "Anual".
    pub label: String,
    /// Mensalidade efetiva (R$/mês).
    pub monthly_price: Decimal,
    /// Valor cobrado por ciclo (mensalidade × meses).
    pub total_per_charge: Decimal,
    /// Texto de economia vs Mensal ("ECONOMIZE R$ 10/MÊS"); "" se
    /// não houver desconto.
    pub savings_label: String,
    /// "MELHOR VALOR" no Anual; "" nos demais.
    pub highlight_label: String,
    /// Descrição curta usada no card.
    pub description: String,
}
