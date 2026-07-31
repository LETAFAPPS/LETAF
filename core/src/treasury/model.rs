use std::fmt;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::BaseFields;

/// Carteira do estabelecimento (tesouraria).
///
/// Regras aplicadas (AI_RULES.md §6, §11):
/// - SINGLETON por empresa: unique em `company_id` na migration; o
///   service rejeita a criação de uma segunda carteira.
/// - `initial_balance` é o saldo inicial declarado pelo operador ao
///   abrir a carteira (`>= 0`, validado no service).
/// - `reserve_goal` é a meta de reserva mensal do caixa (`>= 0`,
///   validado no service; `0` significa "sem meta").
/// - `notes` é observação livre do operador (opcional).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Treasury {
    #[serde(flatten)]
    pub base: BaseFields,
    /// Saldo inicial do estabelecimento.
    pub initial_balance: Decimal,
    #[serde(default)]
    pub notes: Option<String>,
    /// Meta de reserva mensal do caixa da empresa (0 = sem meta).
    ///
    /// `#[serde(default)]` mantém compatibilidade com payloads antigos
    /// do sync (instâncias que ainda não conhecem o campo).
    #[serde(default)]
    pub reserve_goal: Decimal,
}

impl Treasury {
    pub fn new(company_id: Uuid, initial_balance: Decimal, notes: Option<String>) -> Self {
        // Singleton por empresa: id derivado do `company_id` para dois
        // terminais offline não criarem duas tesourarias (§7).
        let mut base = BaseFields::new(company_id);
        base.id = crate::deterministic_id::treasury_account(company_id);
        Self {
            base,
            initial_balance,
            notes,
            reserve_goal: Decimal::ZERO,
        }
    }
}

/// Tipo de movimento MANUAL na carteira do estabelecimento.
///
/// - `Deposit`: aporte de dinheiro no caixa da empresa.
/// - `Withdraw`: retirada de dinheiro do caixa da empresa.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TreasuryMovementKind {
    Deposit,
    Withdraw,
}

impl fmt::Display for TreasuryMovementKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Deposit => write!(f, "deposit"),
            Self::Withdraw => write!(f, "withdraw"),
        }
    }
}

impl TreasuryMovementKind {
    /// Converte o texto persistido no banco de volta para o enum.
    /// Valor desconhecido cai em `Deposit` (mesma tolerância do
    /// `WalletMovementKind`: o sync não quebra por um kind inesperado).
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "withdraw" => Self::Withdraw,
            _ => Self::Deposit,
        }
    }

    /// Sinal do movimento sobre o caixa: `+1` entrada, `-1` saída.
    pub fn sign(self) -> Decimal {
        match self {
            Self::Deposit => Decimal::ONE,
            Self::Withdraw => Decimal::NEGATIVE_ONE,
        }
    }
}

/// Movimento manual da carteira do estabelecimento — append-only no
/// service (UPDATE só para marcar `synced`).
///
/// Regras aplicadas (AI_RULES.md §6, §8, §11):
/// - `amount` é SEMPRE positivo; a direção vem do `kind` — validado no
///   service, nunca confiando no frontend.
/// - `notes` é observação livre do operador (opcional).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreasuryMovement {
    #[serde(flatten)]
    pub base: BaseFields,
    pub treasury_id: Uuid,
    pub kind: TreasuryMovementKind,
    /// Valor do movimento — sempre positivo.
    pub amount: Decimal,
    #[serde(default)]
    pub notes: Option<String>,
}

impl TreasuryMovement {
    pub fn new(
        company_id: Uuid,
        treasury_id: Uuid,
        kind: TreasuryMovementKind,
        amount: Decimal,
        notes: Option<String>,
    ) -> Self {
        Self {
            base: BaseFields::new(company_id),
            treasury_id,
            kind,
            amount,
            notes,
        }
    }
}
