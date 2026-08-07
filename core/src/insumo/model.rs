//! Insumo (matéria-prima): item de estoque CONSUMÍVEL, usado na receita de
//! produtos. Não é vendido diretamente (não aparece no PDV/cardápio) — tem
//! estoque próprio que baixa conforme os produtos que o utilizam são vendidos.
//!
//! Entidade sincronizada (AI_RULES §6/§7): id UUID, `company_id`, timestamps,
//! soft delete e `synced`. O estoque evolui por um ledger append-only
//! ([`InsumoMovement`]) — deltas comutativos, idempotentes por id no sync,
//! espelhando `product::stock_movement`.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::BaseFields;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Insumo {
    #[serde(flatten)]
    pub base: BaseFields,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// Unidade de medida (ex.: "kg", "g", "un", "L", "ml").
    pub unit: String,
    /// Quantidade em estoque (materializada; evolui pelo ledger).
    #[serde(default)]
    pub stock_quantity: f64,
    /// Estoque mínimo para o alerta de reposição.
    #[serde(default)]
    pub min_stock: f64,
    /// Custo unitário (opcional) — apoia o custo de ficha técnica.
    #[serde(default)]
    pub cost_price: Option<Decimal>,
    #[serde(default)]
    pub barcode: Option<String>,
    /// Insumo ativo. Inativo não some do estoque, mas some das listas de
    /// seleção de receita.
    #[serde(default = "default_true")]
    pub active: bool,
}

fn default_true() -> bool {
    true
}

impl Insumo {
    pub fn new(company_id: Uuid, name: String, unit: String) -> Self {
        Self {
            base: BaseFields::new(company_id),
            name,
            description: None,
            unit,
            stock_quantity: 0.0,
            min_stock: 0.0,
            cost_price: None,
            barcode: None,
            active: true,
        }
    }
}

/// Movimento de estoque de insumo — livro-razão append-only (§6/§7).
///
/// Cada alteração de `Insumo::stock_quantity` grava um movimento com o `delta`
/// aplicado (positivo = entrada, negativo = saída/consumo) na MESMA transação
/// que atualiza o valor materializado. Idempotente por `id` no sync; deltas
/// comutativos evitam overselling (ao contrário do LWW sobre o absoluto).
/// Espelha `product::stock_movement::StockMovement`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsumoMovement {
    #[serde(flatten)]
    pub base: BaseFields,
    pub insumo_id: Uuid,
    /// Variação aplicada ao estoque: > 0 entrada, < 0 saída.
    pub delta: f64,
    /// Origem do movimento: "adjust" (ajuste manual), "consumo" (baixa de
    /// funcionário), "recipe" (consumo por venda de produto), etc.
    pub reason: String,
    /// Pedido associado, quando o movimento decorre de uma venda (receita).
    #[serde(default)]
    pub order_id: Option<Uuid>,
}

impl InsumoMovement {
    pub fn new(
        company_id: Uuid,
        insumo_id: Uuid,
        delta: f64,
        reason: impl Into<String>,
        order_id: Option<Uuid>,
    ) -> Self {
        Self {
            base: BaseFields::new(company_id),
            insumo_id,
            delta,
            reason: reason.into(),
            order_id,
        }
    }
}
