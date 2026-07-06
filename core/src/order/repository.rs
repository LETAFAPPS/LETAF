use async_trait::async_trait;
use chrono::NaiveDateTime;
use uuid::Uuid;

use super::model::{Order, OrderStatus};
use crate::error::CoreError;

/// Trait de acesso a dados para Order + OrderItem.
///
/// Regras aplicadas (AI_RULES.md §10):
/// - Acesso ao banco somente via repository
/// - Usar traits para abstração
///
/// O repository gerencia Order e OrderItem juntos
/// (pedido sempre inclui seus itens).
#[async_trait]
pub trait OrderRepository: Send + Sync {
    /// Retorna o próximo número sequencial para a empresa.
    ///
    /// Regras aplicadas (AI_RULES.md §6, §10, §11):
    /// - Calculado via `MAX(number) + 1` filtrado por `company_id` (nunca
    ///   usa auto-incremento do banco — proibido por §6).
    /// - Isolamento por tenant garante sequência independente por empresa.
    async fn next_number(&self, company_id: Uuid) -> Result<i64, CoreError>;

    /// Cria pedido com seus itens em transação.
    async fn create(&self, order: &Order) -> Result<(), CoreError>;

    /// Cria o pedido + seus itens E baixa o estoque dos produtos numa
    /// ÚNICA transação (AI_RULES.md §4 — venda + baixa de estoque atômicas).
    ///
    /// `stock_deltas` = `(product_id, quantidade_a_decrementar)`, uma
    /// entrada por item (quantidades do mesmo produto aplicadas em
    /// sequência). Produtos com `unlimited_stock` não são decrementados.
    /// Estoque insuficiente → `Validation`; produto inexistente/excluído →
    /// `NotFound`; em qualquer erro a transação é revertida (nada é
    /// persistido), dispensando rollback manual de estoque.
    async fn create_atomic(
        &self,
        order: &Order,
        stock_deltas: &[(Uuid, f64)],
    ) -> Result<(), CoreError>;

    /// Busca pedido por ID (com itens).
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Order>, CoreError>;

    /// Lista todos os pedidos de uma empresa (sem itens, para listagem).
    async fn find_all(&self, company_id: Uuid) -> Result<Vec<Order>, CoreError>;

    /// Lista pedidos de um cliente específico (sem itens).
    async fn find_by_customer(&self, company_id: Uuid, customer_id: Uuid) -> Result<Vec<Order>, CoreError>;

    /// Conta usos de um cupom (case-insensitive) em pedidos não-cancelados.
    /// Query dedicada — evita materializar todos os pedidos no checkout.
    async fn count_coupon_uses(&self, company_id: Uuid, coupon_code: &str) -> Result<i64, CoreError>;

    /// Lista pedidos por status.
    async fn find_by_status(&self, company_id: Uuid, status: &OrderStatus) -> Result<Vec<Order>, CoreError>;

    /// Atualiza status do pedido.
    async fn update_status(&self, company_id: Uuid, id: Uuid, status: &OrderStatus) -> Result<(), CoreError>;

    /// Atualiza dados editáveis do pedido em transação:
    /// substitui completamente a lista de itens, reescreve `notes`,
    /// `delivery_type` e `total` (já recalculado pelo service);
    /// marca `synced = false` para o worker propagar.
    ///
    /// Regras aplicadas (AI_RULES.md §6, §10, §11):
    /// - Operação atômica (itens + header juntos).
    /// - `status`, `customer_id`, `coupon_code`, `discount_amount` e
    ///   `number` permanecem intactos — edição é só do "carrinho".
    async fn update(&self, order: &Order) -> Result<(), CoreError>;

    /// Cancela o pedido registrando o motivo.
    ///
    /// Regras aplicadas (AI_RULES.md §6, §11):
    /// - Persiste status = Cancelled e cancellation_reason juntos.
    /// - Marca synced = false para replicação.
    async fn cancel(&self, company_id: Uuid, id: Uuid, reason: &str) -> Result<(), CoreError>;

    /// Soft delete do pedido (e seus itens).
    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError>;

    /// Busca pedidos não sincronizados (com itens).
    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Order>, CoreError>;

    /// Marca pedido como sincronizado.
    async fn mark_synced(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError>;

    /// Upsert de sincronização (§7.7 — last-write-wins via updated_at).
    async fn sync_upsert(&self, order: &Order) -> Result<(), CoreError>;

    /// Busca pedidos atualizados após o timestamp (§7 — sync pull).
    async fn find_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<Order>, CoreError>;
}
