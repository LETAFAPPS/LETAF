use async_trait::async_trait;
use chrono::NaiveDateTime;
use uuid::Uuid;

use super::model::Product;
use super::stock_movement::StockMovement;
use crate::error::CoreError;

/// Trait de acesso a dados para Product.
///
/// Regras aplicadas (AI_RULES.md §10):
/// - Acesso ao banco somente via repository
/// - Usar traits para abstração
///
/// Cada implementação concreta (PostgreSQL, SQLite) ficará
/// na camada correspondente (server/repository, desktop/repository).
#[async_trait]
pub trait ProductRepository: Send + Sync {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Product>, CoreError>;
    async fn find_all(&self, company_id: Uuid) -> Result<Vec<Product>, CoreError>;

    /// Conta os registros ATIVOS da empresa (para o painel do super admin).
    ///
    /// Implementação padrão carrega a lista — suficiente para o SQLite
    /// local, que é pequeno. O PostgreSQL sobrescreve com `COUNT(*)` para
    /// não trazer blobs/linhas inteiras só para contar (§13).
    async fn count_all(&self, company_id: Uuid) -> Result<i64, CoreError> {
        Ok(self.find_all(company_id).await?.len() as i64)
    }

    /// Busca vários produtos por id numa única query (evita N+1, ex.: no
    /// checkout ao validar preços de um carrinho). Ignora ids inexistentes.
    async fn find_by_ids(&self, company_id: Uuid, ids: &[Uuid]) -> Result<Vec<Product>, CoreError>;
    async fn create(&self, product: &Product) -> Result<(), CoreError>;
    async fn update(&self, product: &Product) -> Result<(), CoreError>;
    /// Edição ATÔMICA (§4): metadados + delta de estoque (com ledger append-only)
    /// + associações N:M de adicionais numa ÚNICA transação. Evita o estado
    /// divergente do `update` seguido de `try_adjust_stock`/`replace_addon_groups`
    /// separados, em que uma falha no meio deixava metadados gravados sem o
    /// ajuste de estoque. `stock_delta` é `target - old` (0 = sem mudança);
    /// produto `unlimited_stock` não gera delta nem movimento. Retorna
    /// `Validation` se o delta deixaria o estoque negativo.
    async fn update_atomic(
        &self,
        product: &Product,
        stock_delta: f64,
        addon_group_ids: &[Uuid],
    ) -> Result<(), CoreError>;
    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError>;
    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Product>, CoreError>;
    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError>;

    /// Upsert de sincronização (§7.7 — last-write-wins via updated_at).
    async fn sync_upsert(&self, product: &Product) -> Result<(), CoreError>;

    /// Busca entidades atualizadas após o timestamp (§7 — sync pull).
    async fn find_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<Product>, CoreError>;

    /// Página do pull por KEYSET `(updated_at, id)` (§7, §13): no máximo `limit`
    /// linhas com `(updated_at, id) > (since, after_id)`, ordenadas por
    /// `(updated_at, id)`. Permite paginar bases grandes sem estourar timeout/
    /// memória. Default: delega a `find_updated_since` (sem paginar) — só as
    /// impls que precisam (Postgres) sobrescrevem.
    async fn find_updated_since_paged(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
        _after_id: Uuid,
        _limit: i64,
    ) -> Result<Vec<Product>, CoreError> {
        self.find_updated_since(company_id, since).await
    }

    /// Retorna apenas produtos ativos E visíveis na web — catálogo público.
    /// Filtros aplicados: active = true AND web_visible = true AND deleted_at IS NULL.
    async fn find_active(&self, company_id: Uuid) -> Result<Vec<Product>, CoreError>;

    /// Só o `image_data` do produto (rota de mídia, §13): evita o `SELECT *` e
    /// a query extra de `hydrate_addon_group_ids` do `find_by_id` a cada imagem
    /// servida. Default delega a `find_by_id`; o servidor sobrescreve.
    async fn find_image_data(&self, company_id: Uuid, id: Uuid) -> Result<Option<String>, CoreError> {
        Ok(self.find_by_id(company_id, id).await?.and_then(|p| p.image_data))
    }

    /// Alterna estado ativo/inativo (cardápio web + PDV) — AI_RULES.md §8.
    async fn toggle_active(&self, company_id: Uuid, id: Uuid, active: bool) -> Result<(), CoreError>;

    /// Alterna visibilidade somente no cardápio web — AI_RULES.md §8.
    /// `active` permanece intacto; quando true, o produto ainda é vendido no PDV.
    async fn toggle_web_visible(&self, company_id: Uuid, id: Uuid, visible: bool) -> Result<(), CoreError>;

    /// Lê os IDs dos `AddonGroup` associados ao produto via tabela
    /// `product_addon_groups`. Retorna em ordem `sort_order`.
    async fn find_addon_group_ids(&self, company_id: Uuid, product_id: Uuid) -> Result<Vec<Uuid>, CoreError>;

    /// Substitui completamente as associações do produto pelos
    /// `group_ids` informados, preservando a ordem do vetor como
    /// `sort_order`. Valida `group_id` pertence à empresa via FK no DB.
    async fn replace_addon_groups(
        &self,
        company_id: Uuid,
        product_id: Uuid,
        group_ids: &[Uuid],
    ) -> Result<(), CoreError>;

    /// Aplica `delta` ao estoque em uma única `UPDATE` atômica.
    /// Não toca produtos `unlimited_stock = true` (no-op).
    ///
    /// Retorna `Ok(StockAdjustResult::Adjusted)` quando o UPDATE
    /// alterou exatamente 1 linha; `Unlimited` quando o produto é
    /// ilimitado (sem alteração); `Insufficient` quando o delta
    /// negativo levaria o estoque a < 0; `NotFound` quando o produto
    /// não existe ou está soft-deletado.
    ///
    /// Regras aplicadas (AI_RULES.md §4, §13):
    /// - Substitui o padrão read-modify-write — sem janela de race
    ///   entre `find` e `update` quando dois clientes vendem o mesmo
    ///   produto em paralelo.
    /// - Implementação atualiza `updated_at` para sync (§7).
    async fn try_adjust_stock(
        &self,
        company_id: Uuid,
        product_id: Uuid,
        delta: f64,
    ) -> Result<StockAdjustResult, CoreError>;

    // ── Movimentos de estoque (ledger append-only — AI_RULES §6, §7) ──
    /// Movimentos ainda não sincronizados (push desktop→servidor).
    async fn find_unsynced_stock_movements(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<StockMovement>, CoreError>;

    /// Marca um movimento como sincronizado, condicional ao `updated_at`
    /// empurrado (mesma proteção de §7.6 das demais entidades).
    async fn mark_stock_movement_synced(
        &self,
        company_id: Uuid,
        id: Uuid,
        updated_at: NaiveDateTime,
    ) -> Result<(), CoreError>;

    /// Aplica um movimento recebido de forma IDEMPOTENTE: insere-o (no-op se
    /// o `id` já existe) e, apenas na primeira vez, aplica `stock_quantity +=
    /// delta` ao produto na MESMA transação. Como deltas são comutativos, o
    /// estoque converge sem overselling — ao contrário do LWW sobre o absoluto.
    async fn apply_stock_movement(&self, movement: &StockMovement) -> Result<(), CoreError>;

    /// Movimentos alterados após `since` (pull servidor→desktop).
    async fn find_stock_movements_updated_since(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
    ) -> Result<Vec<StockMovement>, CoreError>;

    /// Página do ledger por keyset `(updated_at, id)` (default delega ao acima;
    /// o ledger cresce sem limite → o Postgres sobrescreve para paginar).
    async fn find_stock_movements_updated_since_paged(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
        _after_id: Uuid,
        _limit: i64,
    ) -> Result<Vec<StockMovement>, CoreError> {
        self.find_stock_movements_updated_since(company_id, since).await
    }
}

/// Resultado da tentativa atômica de ajuste de estoque.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StockAdjustResult {
    /// UPDATE alterou a linha — estoque ajustado.
    Adjusted,
    /// Produto com `unlimited_stock = true` — nenhuma alteração.
    Unlimited,
    /// Delta negativo maior que estoque disponível.
    Insufficient,
    /// Produto não encontrado ou soft-deleted.
    NotFound,
}
