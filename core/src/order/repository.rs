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

    /// Pedidos SEM os itens (`items` vem vazio).
    ///
    /// `find_all` hidrata os itens de TODOS os pedidos — no dataset de teste,
    /// 90 mil linhas de `order_items` para 30 mil pedidos, 404 ms e 34,8 MB.
    /// As telas que listam pedidos (kanban, tesouraria, clientes) mostram
    /// número, cliente, total e status: nenhuma abre os itens, que só
    /// aparecem no DETALHE, carregado um pedido por vez. Sem a hidratação:
    /// 106 ms e 5,8 MB, sem mudar nada do que a tela exibe.
    async fn find_all_light(&self, company_id: Uuid) -> Result<Vec<Order>, CoreError>;

    /// Conta os registros ATIVOS da empresa (para o painel do super admin).
    ///
    /// Implementação padrão carrega a lista — suficiente para o SQLite
    /// local, que é pequeno. O PostgreSQL sobrescreve com `COUNT(*)` para
    /// não trazer blobs/linhas inteiras só para contar (§13).
    async fn count_all(&self, company_id: Uuid) -> Result<i64, CoreError> {
        Ok(self.find_all(company_id).await?.len() as i64)
    }


    /// Página da listagem de operador (mais recentes primeiro), com `limit`/
    /// `offset` — evita materializar TODO o histórico numa listagem que cresce
    /// sem teto (§13). Default delega a `find_all` (desktop lê do SQLite local,
    /// não serve esta rota); o servidor sobrescreve com `LIMIT/OFFSET`.
    async fn find_all_paged(
        &self,
        company_id: Uuid,
        _limit: i64,
        _offset: i64,
    ) -> Result<Vec<Order>, CoreError> {
        self.find_all(company_id).await
    }

    /// Lista pedidos de um cliente específico (sem itens).
    async fn find_by_customer(&self, company_id: Uuid, customer_id: Uuid) -> Result<Vec<Order>, CoreError>;

    /// Conta pedidos em andamento (badge "Pedidos"): tudo que não está
    /// entregue nem cancelado. Padrão carrega a lista; o SQLite
    /// sobrescreve com `COUNT(*)` — o badge roda a cada ciclo de sync e
    /// `find_all` hidrata os ITENS de todos os pedidos. §13.
    async fn count_active(&self, company_id: Uuid) -> Result<i64, CoreError> {
        Ok(self
            .find_all(company_id)
            .await?
            .iter()
            .filter(|o| {
                let s = o.status.to_string();
                s != "delivered" && s != "cancelled"
            })
            .count() as i64)
    }

    /// Conta usos de um cupom (case-insensitive) em pedidos não-cancelados.
    /// Query dedicada — evita materializar todos os pedidos no checkout.
    async fn count_coupon_uses(&self, company_id: Uuid, coupon_code: &str) -> Result<i64, CoreError>;

    /// Conta os pedidos NÃO-cancelados de um cliente. Query dedicada para o
    /// checkout não materializar todo o histórico do cliente só para contar
    /// (§13) — o que também tornaria um `LIMIT` no histórico perigoso para o
    /// limite de cupom.
    async fn count_customer_orders(&self, company_id: Uuid, customer_id: Uuid) -> Result<i64, CoreError>;

    /// Conta quantas vezes um cliente usou um cupom (case-insensitive) em
    /// pedidos não-cancelados. Base do limite por usuário no checkout.
    async fn count_customer_coupon_uses(
        &self,
        company_id: Uuid,
        customer_id: Uuid,
        coupon_code: &str,
    ) -> Result<i64, CoreError>;

    /// Lista pedidos por status.
    /// Pedidos criados no intervalo `[inicio, fim]` (datas inclusivas), com
    /// os itens hidratados.
    ///
    /// Os relatórios carregavam `find_all` — TODO o histórico — e recortavam
    /// o período em Rust. A tela nunca mostra mais que ~2 anos (a janela mais
    /// ampla é "ano" comparado com o ano anterior), então o resto do
    /// histórico era transportado e descartado, e o custo crescia para sempre.
    async fn find_in_period(
        &self,
        company_id: Uuid,
        inicio: chrono::NaiveDate,
        fim: chrono::NaiveDate,
    ) -> Result<Vec<Order>, CoreError>;

    /// Pedidos fiados em ABERTO, de qualquer data — o "a receber" não tem
    /// recorte de período. Conjunto pequeno por natureza.
    async fn find_unpaid_wallet(&self, company_id: Uuid) -> Result<Vec<Order>, CoreError>;

    async fn find_by_status(&self, company_id: Uuid, status: &OrderStatus) -> Result<Vec<Order>, CoreError>;

    /// Atualiza status do pedido.
    async fn update_status(&self, company_id: Uuid, id: Uuid, status: &OrderStatus) -> Result<(), CoreError>;

    /// Atualiza forma de pagamento + status Pago (marca `synced = false`).
    async fn update_payment(
        &self,
        company_id: Uuid,
        id: Uuid,
        payment_method: Option<&str>,
        paid: bool,
    ) -> Result<(), CoreError>;

    /// Atualiza dados editáveis do pedido E ajusta o estoque na MESMA
    /// transação (AI_RULES.md §4, §7.6): substitui a lista de itens, reescreve
    /// `notes`/`delivery_type`/`total` e aplica os deltas de estoque juntos —
    /// sem janela de divergência pedido×estoque na edição.
    ///
    /// `stock_deltas` = `(product_id, delta)` com `delta` = quantidade a SOMAR
    /// ao estoque (negativo baixa quando a edição aumenta a qty; positivo
    /// restitui quando diminui). Estoque insuficiente num delta negativo aborta
    /// a transação (nada é persistido); produto ilimitado/excluído é pulado.
    /// `status`, `customer_id`, `coupon_code`, `number` permanecem intactos.
    async fn update_atomic(
        &self,
        order: &Order,
        stock_deltas: &[(Uuid, f64)],
    ) -> Result<(), CoreError>;

    /// Cancela o pedido E restitui o estoque dos itens na MESMA transação
    /// (AI_RULES.md §4, §7.6 — sem janela de estoque-fantasma se o processo
    /// cair entre o cancelamento e a restituição).
    ///
    /// `restitutions` = `(product_id, quantidade_a_devolver)`, uma entrada por
    /// item. Produtos com `unlimited_stock` ou já excluídos são pulados sem
    /// erro (não há o que restituir) — o cancelamento nunca falha por causa do
    /// estoque. Cada restituição efetivada grava um `StockMovement` (+delta,
    /// razão "cancel") no ledger para propagar via sync idempotente.
    async fn cancel_atomic(
        &self,
        company_id: Uuid,
        id: Uuid,
        reason: &str,
        restitutions: &[(Uuid, f64)],
    ) -> Result<(), CoreError>;

    /// Soft delete do pedido (e seus itens).
    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError>;

    /// Busca pedidos não sincronizados (com itens).
    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Order>, CoreError>;

    /// Marca pedido como sincronizado.
    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError>;

    /// Upsert de sincronização (§7.7 — last-write-wins via updated_at).
    /// Upsert vindo do sync. Devolve `true` quando a versão que chegou
    /// VENCEU o last-write-wins e foi gravada; `false` quando foi
    /// descartada por já existir uma mais nova.
    ///
    /// O chamador precisa saber: um push que "deu 200" mas foi descartado
    /// significa que a edição daquele terminal foi perdida em silêncio —
    /// e o operador tem de ser avisado (§7.6).
    async fn sync_upsert(&self, order: &Order) -> Result<bool, CoreError>;

    /// Busca pedidos atualizados após o timestamp (§7 — sync pull).
    async fn find_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<Order>, CoreError>;

    /// Página do pull por keyset `(updated_at, id)` — o histórico de pedidos
    /// cresce sem limite. Default delega ao acima; só o Postgres sobrescreve.
    /// Cada pedido vem com seus itens (igual `find_updated_since`).
    async fn find_updated_since_paged(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
        _after_id: Uuid,
        _limit: i64,
    ) -> Result<Vec<Order>, CoreError> {
        self.find_updated_since(company_id, since).await
    }
}
