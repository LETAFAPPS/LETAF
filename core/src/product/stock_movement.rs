//! Movimento de estoque — livro-razão append-only (AI_RULES §6, §7).
//!
//! Cada alteração de `Product::stock_quantity` grava um `StockMovement` com
//! o `delta` aplicado (positivo = entrada, negativo = saída) na MESMA
//! transação que atualiza o valor materializado. O sync é idempotente por
//! `base.id`: o servidor aplica `stock_quantity += delta` uma única vez por
//! movimento. Como deltas são comutativos e associativos, duas vendas offline
//! concorrentes do mesmo produto NÃO se sobrescrevem (ao contrário do LWW
//! sobre o valor absoluto). Espelha `CashMovement` / `WalletMovement`.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::BaseFields;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockMovement {
    pub base: BaseFields,
    pub product_id: Uuid,
    /// Variação aplicada ao estoque: > 0 entrada, < 0 saída.
    pub delta: f64,
    /// Origem do movimento: "sale", "sale_edit", "cancel", "manual", etc.
    pub reason: String,
    /// Pedido associado, quando o movimento decorre de uma venda.
    pub order_id: Option<Uuid>,
}

impl StockMovement {
    pub fn new(
        company_id: Uuid,
        product_id: Uuid,
        delta: f64,
        reason: impl Into<String>,
        order_id: Option<Uuid>,
    ) -> Self {
        Self {
            base: BaseFields::new(company_id),
            product_id,
            delta,
            reason: reason.into(),
            order_id,
        }
    }
}
