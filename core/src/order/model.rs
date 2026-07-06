use std::fmt;
use rust_decimal::Decimal;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::BaseFields;

/// Tipo de entrega do pedido.
///
/// Regras aplicadas (AI_RULES.md §6, §8):
/// - Persiste o tipo escolhido pelo cliente no momento da criação.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryType {
    #[default]
    Delivery,
    Pickup,
}

impl fmt::Display for DeliveryType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Delivery => write!(f, "delivery"),
            Self::Pickup   => write!(f, "pickup"),
        }
    }
}

impl DeliveryType {
    /// Decodifica a representação serializada no banco em `DeliveryType`.
    ///
    /// Não implementamos `std::str::FromStr` porque essa função nunca falha
    /// (default = Delivery) — sinal trocado da convenção do trait, que
    /// retorna `Result`. Usar nome próprio evita confusão.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "pickup" => Self::Pickup,
            _        => Self::Delivery,
        }
    }
}

/// Status do pedido — ciclo de vida completo.
///
/// Regras aplicadas (AI_RULES.md §8):
/// - Nomes claros e descritivos
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderStatus {
    Pending,
    Confirmed,
    Preparing,
    Ready,
    Delivered,
    Cancelled,
}

impl fmt::Display for OrderStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Confirmed => write!(f, "confirmed"),
            Self::Preparing => write!(f, "preparing"),
            Self::Ready => write!(f, "ready"),
            Self::Delivered => write!(f, "delivered"),
            Self::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl OrderStatus {
    /// Decodifica a representação serializada no banco em `OrderStatus`.
    ///
    /// Retorna `Option<Self>` (não `Result` do trait `FromStr`) para que o
    /// caller registre warning e use `Pending` como fallback explícito.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "confirmed" => Some(Self::Confirmed),
            "preparing" => Some(Self::Preparing),
            "ready" => Some(Self::Ready),
            "delivered" => Some(Self::Delivered),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

/// Entidade Order — pedido feito pelo cliente final.
///
/// Regras aplicadas (AI_RULES.md §6):
/// - Campos base obrigatórios (UUID, company_id, timestamps, synced)
/// - customer_id vincula ao cliente autenticado
/// - items contém os itens do pedido (carregados pelo repository)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    #[serde(flatten)]
    pub base: BaseFields,
    pub customer_id: Uuid,
    /// Número sequencial do pedido dentro da empresa (começa em 1).
    ///
    /// Regras aplicadas (AI_RULES.md §6, §11):
    /// - Gerado pelo service via `MAX(number) + 1` filtrado por `company_id`.
    /// - **NÃO** é auto-incremento de banco (proibido por §6): o PK continua
    ///   sendo o UUID. Este campo é um "identificador humano" por empresa.
    /// - Isolamento por tenant garante sequência independente por empresa.
    #[serde(default)]
    pub number: i64,
    pub status: OrderStatus,
    /// Total final = soma dos itens − `discount_amount`.
    pub total: Decimal,
    /// Código do cupom aplicado (snapshot). `None` = sem cupom. É o
    /// registro de uso do cupom (sem entidade extra): os limites são
    /// contados a partir dos pedidos não-cancelados com este código
    /// (AI_RULES §6, §11).
    #[serde(default)]
    pub coupon_code: Option<String>,
    /// Valor do desconto aplicado pelo cupom — calculado no servidor,
    /// nunca vindo do frontend (§11). `0.0` = sem desconto.
    #[serde(default)]
    pub discount_amount: Decimal,
    /// Valor adicional/acréscimo aplicado no PDV (taxa, ajuste manual) —
    /// SOMA ao total. Calculado/validado no backend (§11), nunca confiado
    /// do frontend. `0.0` = sem adicional.
    #[serde(default)]
    pub additional_amount: Decimal,
    /// Tipo de entrega escolhido pelo cliente: `delivery` ou `pickup`.
    ///
    /// Regras aplicadas (AI_RULES.md §6, §8):
    /// - Campo obrigatório; defaulta para `delivery` na desserialização.
    #[serde(default)]
    pub delivery_type: DeliveryType,
    pub notes: Option<String>,
    /// Motivo do cancelamento (preenchido somente quando `status == Cancelled`).
    ///
    /// Regras aplicadas (AI_RULES.md §6, §11):
    /// - Obrigatório no momento do cancelamento (validado no service).
    /// - Persistido junto com a transição para `Cancelled` para fins de
    ///   auditoria / rastreabilidade.
    #[serde(default)]
    pub cancellation_reason: Option<String>,
    /// Forma de pagamento (`"cash" | "credit" | "debit" | "pix" | "other"`).
    ///
    /// `None` = pedido sem forma de pagamento registrada (pedidos
    /// vindos do cardápio web não preenchem este campo). Preenchido
    /// no PDV em [`OrderService::create_pdv`] — registro
    /// estruturado pra histórico e relatórios futuros.
    #[serde(default)]
    pub payment_method: Option<String>,
    #[serde(default)]
    pub items: Vec<OrderItem>,
}

impl Order {
    pub fn new(
        company_id: Uuid,
        customer_id: Uuid,
        total: Decimal,
        delivery_type: DeliveryType,
        notes: Option<String>,
    ) -> Self {
        Self {
            base: BaseFields::new(company_id),
            customer_id,
            number: 0,
            status: OrderStatus::Pending,
            total,
            coupon_code: None,
            discount_amount: Decimal::ZERO,
            additional_amount: Decimal::ZERO,
            delivery_type,
            notes,
            cancellation_reason: None,
            payment_method: None,
            items: Vec::new(),
        }
    }
}

/// Formas de pagamento aceitas no PDV. Strings simples (não enum) para
/// flexibilidade ao adicionar novas formas sem migration de schema.
/// `wallet` = consome saldo da carteira do cliente (Fase 12).
pub const PAYMENT_METHODS: &[&str] = &["cash", "credit", "debit", "pix", "wallet", "other"];

/// Item de um pedido — snapshot do produto no momento da compra.
///
/// Regras aplicadas (AI_RULES.md §6):
/// - Campos base obrigatórios
/// - product_name é snapshot (não muda se produto for editado depois)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderItem {
    #[serde(flatten)]
    pub base: BaseFields,
    pub order_id: Uuid,
    pub product_id: Uuid,
    pub product_name: String,
    pub quantity: f64,
    pub unit_price: Decimal,
    pub subtotal: Decimal,
    pub notes: Option<String>,
    /// Snapshot dos adicionais selecionados no carrinho. JSON array
    /// `[{"name": "...", "price": f64}, ...]`. `None` = sem adicionais.
    ///
    /// O `unit_price` já vem do cliente incluindo a soma dos addons —
    /// este campo é só para o PDV mostrar o detalhamento e para futuras
    /// reimpressões/auditoria. Mesmo padrão das demais coleções
    /// embutidas (`availability_schedule`, `discount_tiers`).
    #[serde(default)]
    pub addons_json: Option<String>,
}

impl OrderItem {
    pub fn new(
        company_id: Uuid,
        order_id: Uuid,
        product_id: Uuid,
        product_name: String,
        quantity: f64,
        unit_price: Decimal,
        notes: Option<String>,
        addons_json: Option<String>,
    ) -> Self {
        Self {
            base: BaseFields::new(company_id),
            order_id,
            product_id,
            product_name,
            quantity,
            unit_price,
            subtotal: crate::money::round2(crate::money::qty(quantity) * unit_price),
            notes,
            addons_json,
        }
    }
}
