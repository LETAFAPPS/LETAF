use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use crate::entity::BaseFields;

/// Cupom de desconto aplicável no checkout.
///
/// Regras aplicadas (AI_RULES.md §6, §8, §11):
/// - Campos base obrigatórios (UUID, company_id, timestamps, synced).
/// - `coupon_type` e `discount_kind` validados no service contra
///   allowlists do core (nunca confiar no frontend).
/// - `code` é único por empresa (checado no service via repository).
/// - Valores monetários/limites validados (sem negativos; percent ≤ 100).
/// - `active` controla se o cupom pode ser usado / aparece como ativo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Coupon {
    #[serde(flatten)]
    pub base: BaseFields,
    pub title: String,
    /// Código digitado pelo cliente (normalizado para MAIÚSCULAS, sem
    /// espaços). Único por empresa.
    pub code: String,
    /// `"standard"` | `"first_purchase"` | `"free_shipping"`.
    pub coupon_type: String,
    /// `"fixed"` | `"percent"`.
    pub discount_kind: String,
    /// Valor do desconto: reais (fixed) ou porcentagem 0–100 (percent).
    /// Ignorado quando `coupon_type == "free_shipping"`.
    #[serde(default)]
    pub discount_value: f64,
    /// Valor mínimo de compra para o cupom valer. `0` = sem mínimo.
    #[serde(default)]
    pub min_order_value: f64,
    /// Teto de desconto em reais (relevante para `percent`). `0` = sem teto.
    #[serde(default)]
    pub max_discount: f64,
    /// Limite de usos por mesmo usuário. `0` = ilimitado.
    #[serde(default)]
    pub per_user_limit: i32,
    /// Limite total de usos do cupom. `0` = ilimitado.
    #[serde(default)]
    pub usage_limit: i32,
    /// Início da validade. `None` = válido desde já.
    #[serde(default)]
    pub valid_from: Option<NaiveDateTime>,
    /// Fim da validade. `None` = sem expiração.
    #[serde(default)]
    pub valid_until: Option<NaiveDateTime>,
    #[serde(default = "default_active")]
    pub active: bool,
}

fn default_active() -> bool { true }

impl Coupon {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        company_id: uuid::Uuid,
        title: String,
        code: String,
        coupon_type: String,
        discount_kind: String,
        discount_value: f64,
        min_order_value: f64,
        max_discount: f64,
        per_user_limit: i32,
        usage_limit: i32,
        valid_from: Option<NaiveDateTime>,
        valid_until: Option<NaiveDateTime>,
    ) -> Self {
        Self {
            base: BaseFields::new(company_id),
            title,
            code,
            coupon_type,
            discount_kind,
            discount_value,
            min_order_value,
            max_discount,
            per_user_limit,
            usage_limit,
            valid_from,
            valid_until,
            active: true,
        }
    }
}

impl Coupon {
    /// Desconto monetário deste cupom para um dado `subtotal` (sem
    /// considerar limites de uso/validade — isso é responsabilidade
    /// do `CouponService::evaluate`). `free_shipping` retorna 0.0
    /// porque o pedido ainda não modela taxa de entrega.
    pub fn discount_for(&self, subtotal: f64) -> f64 {
        let raw = match self.coupon_type.as_str() {
            "free_shipping" => 0.0,
            _ => match self.discount_kind.as_str() {
                // Clampa o percentual em [0,100] — defesa em profundidade:
                // o cupom pode entrar via sync (não só por create/update),
                // e um valor inválido daria desconto = subtotal inteiro.
                "percent" => subtotal * self.discount_value.clamp(0.0, 100.0) / 100.0,
                _ => self.discount_value.max(0.0), // "fixed"
            },
        };
        let capped = if self.max_discount > 0.0 {
            raw.min(self.max_discount)
        } else {
            raw
        };
        // Nunca descontar mais que o próprio subtotal.
        capped.min(subtotal).max(0.0)
    }
}

/// Tipos válidos para `Coupon.coupon_type`.
pub const COUPON_TYPES: &[&str] = &["standard", "first_purchase", "free_shipping"];

/// Tipos válidos para `Coupon.discount_kind`.
pub const DISCOUNT_KINDS: &[&str] = &["fixed", "percent"];
