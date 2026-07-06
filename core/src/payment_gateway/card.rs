use async_trait::async_trait;
use rust_decimal::Decimal;

use crate::error::CoreError;

// NOTA (auditoria): `RawCardInput`/`TokenizedCard` + `tokenize_card`
// foram removidos. O fluxo de cartão ativo usa Efi.js no cliente, que
// devolve um `payment_token` — o servidor NUNCA recebe PAN/CVV (fora de
// escopo PCI). Não havia rota usando a tokenização server-side.

/// Dados do titular exigidos pelo antifraude do gateway.
#[derive(Debug, Clone)]
pub struct CardCustomer {
    pub name: String,
    /// CPF/CNPJ só dígitos.
    pub cpf: String,
    pub email: String,
    /// Telefone só dígitos (DDD + número).
    pub phone: String,
    /// Data de nascimento "AAAA-MM-DD" (exigida pelo antifraude Efi).
    pub birth: String,
}

/// Endereço de cobrança exigido pelo antifraude do cartão (one-step).
#[derive(Debug, Clone)]
pub struct CardBillingAddress {
    pub street: String,
    pub number: String,
    pub neighborhood: String,
    pub zipcode: String,
    pub city: String,
    /// UF (2 letras).
    pub state: String,
}

/// Tudo que o gateway precisa para abrir uma assinatura recorrente de
/// cartão. Não carrega PAN/CVV — apenas o `payment_token` já gerado.
#[derive(Debug, Clone)]
pub struct CardSubscriptionInput {
    pub payment_token: String,
    /// Nome do plano exibido na fatura do cartão ("LETAF · Mensal").
    pub plan_name: String,
    /// Descrição do item cobrado por ciclo.
    pub item_name: String,
    /// Valor de cada ciclo em centavos (o gateway trabalha em centavos).
    pub amount_cents: i64,
    /// Intervalo entre cobranças, em meses (1/6/12).
    pub interval_months: u32,
    pub customer: CardCustomer,
    pub billing_address: CardBillingAddress,
    /// URL pública que o gateway chama a cada cobrança (webhook).
    pub notification_url: String,
    /// Identificador interno (nossa `subscription.id`) p/ reconciliar.
    pub custom_id: String,
}

/// Resultado da criação da assinatura no gateway.
#[derive(Debug, Clone)]
pub struct CreatedCardSubscription {
    /// ID da assinatura no gateway (chave de cancelamento/notificação).
    pub gateway_subscription_id: String,
    /// Status do gateway na criação ("active"/"new"/"unpaid"...).
    pub status: String,
    pub card_brand: String,
    pub card_last4: String,
    /// Próxima cobrança agendada pelo gateway, se informada.
    pub next_charge_date: Option<chrono::NaiveDate>,
    /// Status da 1ª cobrança ("paid"/"waiting"/"unpaid"...), se houver.
    pub first_charge_status: Option<String>,
}

/// Status atual da assinatura consultado no gateway (polling da 1ª
/// cobrança e reconciliação).
#[derive(Debug, Clone)]
pub struct CardSubscriptionStatus {
    pub status: String,
    pub next_charge_date: Option<chrono::NaiveDate>,
}

/// Evento de cobrança recebido via notificação do gateway (webhook).
/// O gateway envia um token opaco; a implementação concreta busca os
/// detalhes autenticada e devolve já normalizado.
#[derive(Debug, Clone)]
pub struct CardChargeEvent {
    /// Assinatura do gateway a que o evento pertence.
    pub gateway_subscription_id: String,
    /// Status normalizado da cobrança ("paid"/"unpaid"/"canceled"...).
    pub status: String,
    /// Valor da cobrança em reais.
    pub amount: Decimal,
    /// Quando foi paga (se status = paid).
    pub paid_at: Option<chrono::NaiveDateTime>,
}

/// Trait abstrata do gateway de **cartão recorrente**. Separada do
/// `PaymentGateway` (PIX) porque a responsabilidade é distinta (§8):
/// o motor de assinaturas do gateway dirige a recorrência, não nós.
///
/// Regras aplicadas (AI_RULES.md §1, §11):
/// - Toda chamada HTTP vive na implementação concreta (server).
/// - Entradas/saídas em tipos do domínio — sem JSON cru.
#[async_trait]
pub trait CardGateway: Send + Sync {
    /// Cria plano + assinatura recorrente no gateway. A partir daqui o
    /// gateway cobra sozinho a cada ciclo e nos notifica por webhook.
    async fn create_card_subscription(
        &self,
        input: &CardSubscriptionInput,
    ) -> Result<CreatedCardSubscription, CoreError>;

    /// Consulta o status atual da assinatura (polling da 1ª cobrança).
    async fn fetch_subscription_status(
        &self,
        gateway_subscription_id: &str,
    ) -> Result<CardSubscriptionStatus, CoreError>;

    /// Cancela a assinatura no gateway (encerra a recorrência).
    async fn cancel_subscription(
        &self,
        gateway_subscription_id: &str,
    ) -> Result<(), CoreError>;

    /// Busca os detalhes de uma notificação (token opaco do webhook) e
    /// devolve os eventos de cobrança já normalizados.
    async fn fetch_notification(
        &self,
        token: &str,
    ) -> Result<Vec<CardChargeEvent>, CoreError>;

    /// Nome do gateway (coluna `gateway` em subscriptions).
    fn name(&self) -> &str;
}
