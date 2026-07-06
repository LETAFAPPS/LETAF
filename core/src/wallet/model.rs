use std::fmt;
use rust_decimal::Decimal;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::BaseFields;

/// Tipo de movimentação na carteira.
///
/// - `Deposit`: cliente colocou saldo na carteira (operador
///   registrou um depósito em dinheiro/pix/cartão).
/// - `Withdraw`: cliente sacou saldo da carteira.
/// - `OrderCharge`: pedido pago usando saldo (consome balance).
/// - `OrderRefund`: estorno de pedido — devolve saldo.
/// - `ManualAdjust`: correção manual (auditoria). Operador deve
///   anotar o motivo no `notes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WalletMovementKind {
    Deposit,
    Withdraw,
    OrderCharge,
    OrderRefund,
    ManualAdjust,
}

impl fmt::Display for WalletMovementKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Deposit => write!(f, "deposit"),
            Self::Withdraw => write!(f, "withdraw"),
            Self::OrderCharge => write!(f, "order_charge"),
            Self::OrderRefund => write!(f, "order_refund"),
            Self::ManualAdjust => write!(f, "manual_adjust"),
        }
    }
}

impl WalletMovementKind {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "deposit" => Self::Deposit,
            "withdraw" => Self::Withdraw,
            "order_charge" => Self::OrderCharge,
            "order_refund" => Self::OrderRefund,
            "manual_adjust" => Self::ManualAdjust,
            _ => Self::ManualAdjust,
        }
    }

    /// Sinal do movimento (multiplicador para somar no balance):
    /// `+1` para entrada de saldo, `-1` para saída.
    pub fn sign(self) -> Decimal {
        match self {
            Self::Deposit | Self::OrderRefund => rust_decimal_macros::dec!(1),
            Self::Withdraw | Self::OrderCharge => rust_decimal_macros::dec!(-1),
            // Ajustes manuais: assumimos +1 e o sinal real fica
            // codificado no `amount` (negativo permitido nesse caso).
            // Esta convenção é validada no service.
            Self::ManualAdjust => rust_decimal_macros::dec!(1),
        }
    }
}

/// Conta-carteira de um cliente.
///
/// Regras aplicadas (AI_RULES.md §6, §11):
/// - 1:1 com `Customer`: garantido pela unique index em
///   `(company_id, customer_id)` na migration; service trata
///   "abrir conta" como upsert idempotente.
/// - `balance` pode ser negativo (fiado). Saque que faria
///   `balance < -credit_limit` é rejeitado.
/// - `credit_limit >= 0`. `0.0` significa fiado proibido.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletAccount {
    #[serde(flatten)]
    pub base: BaseFields,
    pub customer_id: Uuid,
    pub balance: Decimal,
    pub credit_limit: Decimal,
}

impl WalletAccount {
    pub fn new(company_id: Uuid, customer_id: Uuid) -> Self {
        Self {
            base: BaseFields::new(company_id),
            customer_id,
            balance: Decimal::ZERO,
            credit_limit: Decimal::ZERO,
        }
    }

    /// Saldo mínimo permitido — service usa para validar saques.
    /// É `-credit_limit` (limite negativo permitido).
    pub fn floor(&self) -> Decimal {
        -self.credit_limit
    }

    /// `true` quando a conta está em fiado (saldo negativo).
    pub fn is_in_debt(&self) -> bool {
        self.balance < Decimal::ZERO
    }
}

/// Movimento da carteira — append-only no service (UPDATE só pra
/// marcar `synced`).
///
/// Regras aplicadas (AI_RULES.md §6, §8):
/// - `amount` é positivo em todos os kinds **exceto** `ManualAdjust`,
///   que aceita negativo (ajuste para baixo). O service valida.
/// - `balance_after` é snapshot pós-operação — usado para auditoria
///   sem precisar replicar/replay todo o histórico.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletMovement {
    #[serde(flatten)]
    pub base: BaseFields,
    pub account_id: Uuid,
    pub kind: WalletMovementKind,
    pub amount: Decimal,
    pub balance_after: Decimal,
    pub related_order_id: Option<Uuid>,
    pub notes: Option<String>,
}

impl WalletMovement {
    pub fn new(
        company_id: Uuid,
        account_id: Uuid,
        kind: WalletMovementKind,
        amount: Decimal,
        balance_after: Decimal,
    ) -> Self {
        Self {
            base: BaseFields::new(company_id),
            account_id,
            kind,
            amount,
            balance_after,
            related_order_id: None,
            notes: None,
        }
    }
}
