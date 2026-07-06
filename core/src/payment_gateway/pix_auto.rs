use async_trait::async_trait;

use crate::error::CoreError;

/// Dados do pagador exigidos para abrir a recorrência (mandato).
#[derive(Debug, Clone)]
pub struct PixAutoCustomer {
    pub name: String,
    /// CPF/CNPJ só dígitos.
    pub cpf: String,
}

/// Tudo que o gateway precisa para criar a recorrência (mandato) de
/// Pix Automático. Valor **fixo** por ciclo (decisão do projeto).
#[derive(Debug, Clone)]
pub struct PixAutoInput {
    /// Valor de cada ciclo em centavos.
    pub amount_cents: i64,
    /// Intervalo entre cobranças, em meses (1/6/12).
    pub interval_months: u32,
    /// Nome exibido ao pagador no app do banco ("LETAF · Mensal").
    pub plan_name: String,
    /// Descrição curta da cobrança.
    pub description: String,
    pub customer: PixAutoCustomer,
    /// URL pública que o gateway chama a cada débito (webhook PIX).
    pub notification_url: String,
    /// Identificador interno (nossa `subscription.id`) p/ reconciliar.
    pub custom_id: String,
}

/// Resultado da criação da recorrência: o **QR de autorização** que o
/// pagador escaneia no app do banco dele para aprovar o mandato.
#[derive(Debug, Clone)]
pub struct CreatedRecurrence {
    /// ID da recorrência no gateway (`idRec`).
    pub rec_id: String,
    /// BR Code (copia-e-cola) de **autorização** do mandato.
    pub copia_cola: String,
    /// PNG do QR Code em base64 (sem o prefixo data-url).
    pub qr_code_b64: String,
    /// Status inicial ("pending"/"criada"/"aguardando autorização").
    pub status: String,
}

/// Status atual da recorrência consultado no gateway (polling da
/// autorização).
#[derive(Debug, Clone)]
pub struct RecurrenceStatus {
    /// Status normalizado: "pending"/"active"/"rejected"/"canceled".
    pub status: String,
    pub next_charge_date: Option<chrono::NaiveDate>,
}

/// Evento de débito recebido via webhook (cobrança recorrente `cobr`).
#[derive(Debug, Clone)]
pub struct PixAutoChargeEvent {
    /// Recorrência (`idRec`) a que o evento pertence.
    pub rec_id: String,
    /// Status normalizado da cobrança ("paid"/"unpaid"/"canceled"...).
    pub status: String,
    /// Valor da cobrança em reais.
    pub amount: f64,
    /// Quando foi liquidada (se paga).
    pub paid_at: Option<chrono::NaiveDateTime>,
}

/// Status de recorrência considerados **autorizada/ativa**.
pub fn is_active_status(s: &str) -> bool {
    matches!(s, "active" | "ativa" | "approved" | "aprovada" | "authorized")
}

/// Status de recorrência considerados **recusada/encerrada**.
pub fn is_rejected_status(s: &str) -> bool {
    matches!(
        s,
        "rejected" | "rejeitada" | "canceled" | "cancelled" | "cancelada" | "expired" | "expirada"
    )
}

/// Trait abstrata do **Pix Automático** (débito recorrente do Banco
/// Central). Separada do `PaymentGateway` (PIX imediato) e do
/// `CardGateway` porque a responsabilidade é distinta (§8): aqui há um
/// mandato autorizado pelo pagador e o banco dele debita sozinho.
///
/// Regras aplicadas (AI_RULES.md §1, §11):
/// - Toda chamada HTTP vive na implementação concreta (server, reusa o
///   `EfiClient` com mTLS da API PIX).
/// - Entradas/saídas em tipos do domínio — sem JSON cru.
#[async_trait]
pub trait PixAutoGateway: Send + Sync {
    /// Cria a recorrência (mandato) e devolve o QR de autorização para
    /// o pagador aprovar no app do banco dele.
    async fn create_recurrence(
        &self,
        input: &PixAutoInput,
    ) -> Result<CreatedRecurrence, CoreError>;

    /// Consulta o status da recorrência (polling da autorização).
    async fn fetch_recurrence_status(
        &self,
        rec_id: &str,
    ) -> Result<RecurrenceStatus, CoreError>;

    /// Cria uma cobrança recorrente (`cobr`) de um ciclo. O banco do
    /// pagador debita automaticamente no vencimento.
    async fn create_recurring_charge(
        &self,
        rec_id: &str,
        amount_cents: i64,
        due_date: chrono::NaiveDate,
        description: &str,
        custom_id: &str,
    ) -> Result<(), CoreError>;

    /// Cancela a recorrência (encerra o mandato).
    async fn cancel_recurrence(&self, rec_id: &str) -> Result<(), CoreError>;

    /// Interpreta o corpo do webhook PIX (a API PIX envia o payload
    /// diretamente, validado por mTLS) e devolve os eventos de débito
    /// já normalizados.
    fn parse_webhook(&self, body: &str) -> Result<Vec<PixAutoChargeEvent>, CoreError>;

    /// Nome do gateway (coluna `gateway` em subscriptions).
    fn name(&self) -> &str;
}
