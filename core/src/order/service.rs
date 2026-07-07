use std::sync::Arc;
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use uuid::Uuid;

use crate::money;

use super::model::{DeliveryType, Order, OrderItem, OrderStatus};
use super::repository::OrderRepository;
use crate::addon::service::AddonService;
use crate::cash::service::CashService;
use crate::error::CoreError;
use crate::product::service::ProductService;

/// Dados de entrada para um item do pedido.
pub struct OrderItemInput {
    pub product_id: Uuid,
    pub product_name: String,
    pub quantity: f64,
    /// Preço unitário JÁ COM addons somados (cliente calcula). O
    /// `addons_json` carrega o detalhamento para o operador no PDV.
    pub unit_price: Decimal,
    pub notes: Option<String>,
    pub addons_json: Option<String>,
}

/// Service para o domínio Order.
///
/// Regras aplicadas (AI_RULES.md §1, §9, §11):
/// - service.rs contém a orquestração de regras de negócio
/// - Depende de repository via trait (inversão de dependência)
/// - Validar todos os dados de entrada
pub struct OrderService {
    repo: Arc<dyn OrderRepository>,
    product_service: Arc<ProductService>,
    /// Opcional: quando presente, vendas via `create_pdv` com `session_id`
    /// disparam o registro de `CashMovement::Sale` na sessão de caixa.
    /// Mantido `Option` porque o server não usa essa integração (PDV é
    /// exclusivo do desktop).
    cash_service: Option<Arc<CashService>>,
    /// Opcional: quando presente (servidor), revalida o preço dos
    /// adicionais contra o catálogo do tenant (§11) em vez de confiar no
    /// `addons_json` enviado pelo cliente. Ausente no desktop (PDV é
    /// operado por usuário autenticado e confiável, offline).
    addon_service: Option<Arc<AddonService>>,
}

impl OrderService {
    pub fn new(repo: Arc<dyn OrderRepository>, product_service: Arc<ProductService>) -> Self {
        Self { repo, product_service, cash_service: None, addon_service: None }
    }

    /// Builder fluente para injetar o `CashService` quando o PDV
    /// precisa registrar vendas na sessão ativa (desktop).
    pub fn with_cash_service(mut self, cash: Arc<CashService>) -> Self {
        self.cash_service = Some(cash);
        self
    }

    /// Builder fluente para injetar o `AddonService` (servidor) e
    /// revalidar o preço dos adicionais contra o catálogo (§11).
    pub fn with_addon_service(mut self, addons: Arc<AddonService>) -> Self {
        self.addon_service = Some(addons);
        self
    }

    /// Cria um pedido a partir de dados brutos.
    ///
    /// Valida itens, calcula total, constrói entidade e persiste.
    ///
    /// Regras aplicadas (AI_RULES.md §1, §4, §6, §9, §11):
    /// - Número sequencial por empresa via `MAX(number)+1` (não auto-incremento).
    /// - Retry automático (até 3 tentativas) em colisão de número sequencial:
    ///   garante resiliência à race condition TOCTOU sem abrir transação distribuída.
    ///   O UNIQUE INDEX em (company_id, number) na migração 020/019 é a garantia
    ///   de integridade; este retry garante que o cliente não receba erro 500.
    /// - **Estoque**: cada item gera uma baixa imediata em `stock_quantity`.
    ///   Se algum item não tiver estoque, todas as baixas já feitas são
    ///   restituídas antes de retornar o erro (rollback manual sem transação
    ///   distribuída). Isso falha cedo, antes mesmo de tentar persistir o pedido.
    pub async fn create(
        &self,
        company_id: Uuid,
        customer_id: Uuid,
        items: Vec<OrderItemInput>,
        delivery_type: DeliveryType,
        notes: Option<String>,
        coupon_code: Option<String>,
        discount_amount: Decimal,
    ) -> Result<Order, CoreError> {
        validate_items(&items)?;
        self.verify_item_prices(company_id, &items).await?;

        let order_id = Uuid::new_v4();
        let (final_items, items_total) = build_items(company_id, order_id, &items);
        // Desconto vem JÁ calculado/validado pelo caller (server),
        // que recomputa via CouponService — nunca confiamos no valor
        // do frontend (§11). Aqui só garantimos que não fica negativo.
        let (discount, total) = order_total(items_total, discount_amount, Decimal::ZERO);
        // Quantidade a decrementar por item (§4: aplicada na MESMA
        // transação do insert do pedido, dentro de `create_atomic`).
        let stock_deltas: Vec<(Uuid, f64)> =
            items.iter().map(|i| (i.product_id, i.quantity)).collect();

        // Aumentado de 3 para 10: com 4+ operadores no PDV criando
        // pedidos no mesmo segundo, 3 retries era insuficiente —
        // o quarto cliente concorrente recebia erro 500 mesmo com
        // sequência válida disponível. Como cada `next_number +
        // create` envolve I/O (~5-15ms), 10 retries cobrem cenários
        // realistas sem adicionar latência perceptível ao caso feliz.
        const MAX_RETRIES: u8 = 10;
        for attempt in 0..MAX_RETRIES {
            let mut order = Order::new(company_id, customer_id, total, delivery_type.clone(), notes.clone());
            order.base.id = order_id;
            order.coupon_code = coupon_code.clone();
            order.discount_amount = discount;
            order.number = self.repo.next_number(company_id).await?;
            order.items = final_items.clone();
            // Colisão de número sequencial reverte a tx (estoque incluso)
            // e tenta de novo — sem rollback manual de estoque.
            match self.repo.create_atomic(&order, &stock_deltas).await {
                Ok(()) => return Ok(order),
                Err(CoreError::Repository(ref msg))
                    if attempt + 1 < MAX_RETRIES && is_unique_violation(msg) =>
                {
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
        Err(CoreError::Repository("Failed to assign unique order number after retries".into()))
    }

    /// Cria um pedido a partir do PDV (Ponto de Venda local).
    ///
    /// Diferenças vs [`Self::create`]:
    /// - Status inicial depende do `delivery_type`:
    ///   * `Pickup` (balcão): `Delivered` — venda já finalizada, não
    ///     entra no fluxo de cozinha (estoque já saiu; cliente leva).
    ///   * `Delivery` (entrega): `Preparing` — pula `Pending` (sem
    ///     alarme) e vai direto pro preparo, porque o operador criou
    ///     o pedido pessoalmente.
    ///     (Pedidos de mesa, quando implementados, entrarão como
    ///     `Preparing` também — fluxo se ramifica aqui no service.)
    /// - `payment_method` registrado no pedido para histórico.
    /// - `customer_id` pode ser `Uuid::nil()` (venda balcão anônima).
    /// - Sem cupom (não suportado nesta fase do PDV).
    /// - Demais regras (baixa de estoque com rollback, retry de
    ///   `next_number`, validações) idênticas ao `create`.
    pub async fn create_pdv(
        &self,
        company_id: Uuid,
        customer_id: Uuid,
        items: Vec<OrderItemInput>,
        discount_amount: Decimal,
        additional_amount: Decimal,
        delivery_type: DeliveryType,
        payment_method: Option<String>,
        notes: Option<String>,
        session_id: Option<Uuid>,
    ) -> Result<Order, CoreError> {
        validate_items(&items)?;
        if let Some(method) = payment_method.as_deref() {
            if !crate::order::model::PAYMENT_METHODS.contains(&method) {
                return Err(CoreError::Validation(format!(
                    "Unknown payment method '{method}'"
                )));
            }
        }
        self.verify_item_prices(company_id, &items).await?;

        let order_id = Uuid::new_v4();
        let (final_items, items_total) = build_items(company_id, order_id, &items);
        // §11: backend recomputa. Desconto clampado a [0, itens];
        // adicional (acréscimo) não-negativo soma ao total.
        let additional = additional_amount.max(Decimal::ZERO);
        let (discount, total) = order_total(items_total, discount_amount, additional);
        // Baixa de estoque na MESMA transação do insert (§4).
        let stock_deltas: Vec<(Uuid, f64)> =
            items.iter().map(|i| (i.product_id, i.quantity)).collect();

        let initial_status = match delivery_type {
            DeliveryType::Pickup => OrderStatus::Delivered,
            DeliveryType::Delivery => OrderStatus::Preparing,
        };

        const MAX_RETRIES: u8 = 10;
        for attempt in 0..MAX_RETRIES {
            let mut order = Order::new(
                company_id,
                customer_id,
                total,
                delivery_type.clone(),
                notes.clone(),
            );
            order.base.id = order_id;
            order.status = initial_status.clone();
            order.payment_method = payment_method.clone();
            order.discount_amount = discount;
            order.additional_amount = additional;
            order.number = self.repo.next_number(company_id).await?;
            order.items = final_items.clone();
            match self.repo.create_atomic(&order, &stock_deltas).await {
                Ok(()) => {
                    // Lança movimento na sessão de caixa, se houver.
                    // Falha NÃO desfaz a venda — pedido é fonte de
                    // verdade; movimento é só reflexo agregado.
                    //
                    // EXCEÇÃO: `payment_method == "wallet"` não passa
                    // pelo caixa físico (saldo do cliente cobre); a
                    // contabilização vira `WalletMovement::OrderCharge`
                    // disparado pelo controller PDV após este retorno.
                    if let (Some(cash), Some(sid), Some(method)) =
                        (&self.cash_service, session_id, order.payment_method.as_ref())
                    {
                        if method != "wallet" {
                            if let Err(e) = cash
                                .register_sale_movement(
                                    company_id,
                                    sid,
                                    order.base.id,
                                    order.total,
                                    method.clone(),
                                )
                                .await
                            {
                                tracing::warn!(
                                    "PDV order {} created but cash movement failed: {e}",
                                    order.base.id
                                );
                            }
                        }
                    }
                    return Ok(order);
                }
                Err(CoreError::Repository(ref msg))
                    if attempt + 1 < MAX_RETRIES && is_unique_violation(msg) =>
                {
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
        Err(CoreError::Repository("Failed to assign unique order number after retries".into()))
    }

    pub async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Order>, CoreError> {
        self.repo.find_by_id(company_id, id).await
    }

    pub async fn find_all(&self, company_id: Uuid) -> Result<Vec<Order>, CoreError> {
        self.repo.find_all(company_id).await
    }

    pub async fn find_by_customer(&self, company_id: Uuid, customer_id: Uuid) -> Result<Vec<Order>, CoreError> {
        self.repo.find_by_customer(company_id, customer_id).await
    }

    /// Conta usos de um cupom (não-cancelados) por query dedicada —
    /// evita carregar todos os pedidos no checkout (§13).
    pub async fn count_coupon_uses(&self, company_id: Uuid, coupon_code: &str) -> Result<i64, CoreError> {
        self.repo.count_coupon_uses(company_id, coupon_code).await
    }

    pub async fn find_by_status(&self, company_id: Uuid, status: &OrderStatus) -> Result<Vec<Order>, CoreError> {
        self.repo.find_by_status(company_id, status).await
    }

    /// Avança o pedido para o próximo status no fluxo e retorna o pedido atualizado.
    ///
    /// Regras aplicadas (AI_RULES.md §1, §8, §9):
    /// - Lógica de transição de status centralizada no service (§1 — não na UI).
    /// - Usa apenas 2 queries (find_by_id + update_status), atualizando status
    ///   em memória para evitar round-trip extra (N+1).
    /// - Pedidos em estado terminal (Delivered, Cancelled) são retornados sem
    ///   alteração — sem erro.
    pub async fn advance_status(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<Option<Order>, CoreError> {
        let mut order = match self.repo.find_by_id(company_id, id).await? {
            Some(o) => o,
            None => return Ok(None),
        };
        let next = match order.status {
            OrderStatus::Pending    => Some(OrderStatus::Confirmed),
            OrderStatus::Confirmed  => Some(OrderStatus::Preparing),
            OrderStatus::Preparing  => Some(OrderStatus::Ready),
            OrderStatus::Ready      => Some(OrderStatus::Delivered),
            OrderStatus::Delivered | OrderStatus::Cancelled => None,
        };
        if let Some(next_status) = next {
            self.repo.update_status(company_id, id, &next_status).await?;
            order.status = next_status;
        }
        Ok(Some(order))
    }

    /// Atualiza o status do pedido, validando a transição, e retorna o
    /// pedido atualizado.
    ///
    /// Regras aplicadas (AI_RULES.md §1, §8, §11 — máquina de estados no
    /// service, não na UI; o backend não confia no valor do frontend):
    /// - `Cancelled` NÃO é aceito por aqui: cancelar tem fluxo próprio
    ///   ([`Self::cancel`]) que exige motivo e devolve o estoque. Aceitar
    ///   `Cancelled` aqui marcaria o pedido cancelado sem motivo e sem
    ///   restituir estoque (bug de integridade).
    /// - Estados terminais (`Delivered`, `Cancelled`) não podem ser reabertos.
    /// - Retorna o Order atualizado sem round-trip extra (reusa o carregado).
    pub async fn update_status(
        &self,
        company_id: Uuid,
        id: Uuid,
        status: OrderStatus,
    ) -> Result<Order, CoreError> {
        if status == OrderStatus::Cancelled {
            return Err(CoreError::Validation(
                "Para cancelar um pedido use o cancelamento (com motivo)".into(),
            ));
        }
        let mut order = self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("Order not found".into()))?;
        if matches!(order.status, OrderStatus::Delivered | OrderStatus::Cancelled) {
            return Err(CoreError::Validation(format!(
                "Pedido em estado terminal ({}) não pode mudar de status",
                order.status
            )));
        }
        self.repo.update_status(company_id, id, &status).await?;
        order.status = status;
        Ok(order)
    }

    /// Edita um pedido existente: substitui a lista de itens completa,
    /// atualiza `notes`, `delivery_type` e recomputa o total.
    ///
    /// `new_items` é a lista FINAL de itens (existentes + novos +
    /// reordenados). Cada item carrega:
    /// - `base.id`: zero (`Uuid::nil`) para itens novos — o service
    ///   gera UUID novo. Caso contrário, preserva.
    /// - `quantity`, `unit_price`, `product_id`, `product_name`,
    ///   `notes`, `addons_json`: dados do snapshot.
    ///
    /// O service recomputa `subtotal = quantity × unit_price` para
    /// cada item e o total geral.
    ///
    /// Regras aplicadas (AI_RULES.md §1, §6, §11):
    /// - Pedidos finalizados (`Delivered`/`Cancelled`) não podem ser
    ///   editados — operações irreversíveis.
    /// - Lista vazia é erro: pedido sem itens não faz sentido.
    /// - Total = soma dos subtotais − `discount_amount` (preservado).
    /// - Estoque é reajustado pelo delta (qty antiga − nova) por produto,
    ///   best-effort após o update (mesmo padrão do `cancel`): venda que
    ///   diminui restitui estoque; que aumenta dá baixa adicional.
    pub async fn update_basics(
        &self,
        company_id: Uuid,
        id: Uuid,
        new_items: Vec<super::model::OrderItem>,
        notes: Option<String>,
        delivery_type: super::model::DeliveryType,
    ) -> Result<Order, CoreError> {
        let mut order = self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("Order not found".into()))?;
        match order.status {
            OrderStatus::Delivered => return Err(CoreError::Validation(
                "Pedidos entregues não podem ser editados".into(),
            )),
            OrderStatus::Cancelled => return Err(CoreError::Validation(
                "Pedidos cancelados não podem ser editados".into(),
            )),
            _ => {}
        }
        if new_items.is_empty() {
            return Err(CoreError::Validation(
                "Pedido precisa ter ao menos um item".into(),
            ));
        }
        // §11: nunca confiar no preço vindo do cliente — reconfere cada
        // `unit_price` contra o catálogo (igual ao `create`), inclusive
        // na EDIÇÃO de pedido. Sem isto, uma requisição forjada poderia
        // reescrever os preços para qualquer valor.
        let price_check: Vec<OrderItemInput> = new_items
            .iter()
            .map(|it| OrderItemInput {
                product_id: it.product_id,
                product_name: it.product_name.clone(),
                quantity: it.quantity,
                unit_price: it.unit_price,
                notes: it.notes.clone(),
                addons_json: it.addons_json.clone(),
            })
            .collect();
        self.verify_item_prices(company_id, &price_check).await?;

        let now = chrono::Utc::now().naive_utc();
        let mut finalized: Vec<super::model::OrderItem> = Vec::with_capacity(new_items.len());
        for mut it in new_items.into_iter() {
            if it.quantity <= 0.0 {
                return Err(CoreError::Validation(
                    "Quantidade de item deve ser positiva".into(),
                ));
            }
            if it.base.id == Uuid::nil() {
                // Item novo — gera UUID e marca campos base.
                it.base = crate::entity::BaseFields::new(company_id);
            } else {
                it.base.updated_at = now;
                it.base.synced = false;
            }
            it.order_id = id;
            it.subtotal = money::round2(money::qty(it.quantity) * it.unit_price);
            finalized.push(it);
        }
        let subtotal: Decimal = finalized.iter().map(|i| i.subtotal).sum();
        // Preserva desconto E adicional ao recompor o total (§11). Reatribui o
        // desconto CLAMPADO: se a edição derrubou o subtotal abaixo do desconto
        // antigo, `discount_amount` não pode continuar maior que os itens.
        let (new_discount, new_total) =
            order_total(subtotal, order.discount_amount, order.additional_amount);
        order.discount_amount = new_discount;

        // Delta de estoque por produto = soma das qty ANTIGAS − soma das
        // qty NOVAS. Calculado antes de sobrescrever `order.items`.
        let mut stock_delta: std::collections::HashMap<Uuid, f64> = std::collections::HashMap::new();
        for it in &order.items {
            *stock_delta.entry(it.product_id).or_insert(0.0) += it.quantity;
        }
        for it in &finalized {
            *stock_delta.entry(it.product_id).or_insert(0.0) -= it.quantity;
        }

        order.items = finalized;
        order.notes = notes;
        order.delivery_type = delivery_type;
        order.total = new_total;
        order.base.updated_at = now;
        order.base.synced = false;

        self.repo.update(&order).await?;
        // Reajuste best-effort do estoque (mesmo padrão do `cancel`): falha
        // é logada, nunca silenciosa (§7.6). delta > 0 restitui; < 0 baixa.
        for (product_id, delta) in stock_delta {
            if delta.abs() < f64::EPSILON {
                continue;
            }
            if let Err(e) = self
                .product_service
                .adjust_stock(company_id, product_id, delta)
                .await
            {
                tracing::warn!(
                    "Edit order {}: stock adjust failed for product {} (delta {}): {}",
                    id, product_id, delta, e
                );
            }
        }
        self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("Order not found".into()))
    }

    /// Cancela um pedido registrando o motivo (obrigatório) e retorna o pedido.
    ///
    /// Regras aplicadas (AI_RULES.md §1, §6, §8, §9, §11):
    /// - Motivo é obrigatório e não pode ser vazio (validação no service).
    /// - Pedidos já entregues ou já cancelados não podem ser cancelados.
    /// - **Estoque**: a transição "não-cancelado → cancelado" restitui a
    ///   quantidade de cada item. Como só passamos por aqui uma vez por
    ///   pedido (status `Cancelled` é estado terminal), a restituição é
    ///   idempotente por construção.
    /// - Retorna o Order atualizado para evitar round-trip extra no handler (N+1).
    pub async fn cancel(
        &self,
        company_id: Uuid,
        id: Uuid,
        reason: &str,
    ) -> Result<Order, CoreError> {
        let trimmed = reason.trim();
        if trimmed.is_empty() {
            return Err(CoreError::Validation(
                "Cancellation reason is required".into(),
            ));
        }
        let order = self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("Order not found".into()))?;
        match order.status {
            OrderStatus::Delivered => {
                return Err(CoreError::Validation(
                    "Delivered orders cannot be cancelled".into(),
                ));
            }
            OrderStatus::Cancelled => {
                return Err(CoreError::Validation("Order already cancelled".into()));
            }
            _ => {}
        }
        // Cancela E restitui o estoque na MESMA transação (§4, §7.6): sem
        // janela de estoque-fantasma se o processo cair no meio. Produtos
        // ilimitados/excluídos são pulados pelo repo (o cancel não falha por
        // estoque). O ledger idempotente propaga a devolução via sync.
        let restitutions: Vec<(Uuid, f64)> = order
            .items
            .iter()
            .map(|item| (item.product_id, item.quantity))
            .collect();
        self.repo.cancel_atomic(company_id, id, trimmed, &restitutions).await?;
        self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("Order not found".into()))
    }

    pub async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("Order not found".into()))?;
        self.repo.soft_delete(company_id, id).await
    }

    pub async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Order>, CoreError> {
        self.repo.find_unsynced(company_id).await
    }

    pub async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        self.repo.mark_synced(company_id, id, updated_at).await
    }

    pub async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
    ) -> Result<Vec<Order>, CoreError> {
        self.repo.find_updated_since(company_id, since).await
    }

    pub async fn sync_upsert(&self, company_id: Uuid, mut order: Order) -> Result<(), CoreError> {
        if order.base.company_id != company_id {
            return Err(CoreError::Validation("Company mismatch".into()));
        }
        order.base.synced = true;
        for item in &mut order.items {
            item.base.synced = true;
        }
        self.repo.sync_upsert(&order).await
    }

    /// Confere o `unit_price` informado contra o preço persistido do produto.
    ///
    /// Regras aplicadas (AI_RULES.md §11):
    /// - "Nunca confiar em dados vindos do frontend": preço enviado pelo
    ///   cliente pode ser manipulado para criar venda com lucro negativo.
    /// - Validação tolera um `epsilon` de centavo para absorver erros de
    ///   ponto flutuante; descontos `bulk_*` adicionam o tier vencedor
    ///   ao preço esperado conforme `quantity` enviada.
    /// - Adicionais: quando `addon_service` está injetado (servidor), o
    ///   preço de cada adicional é resolvido pelo `id` no catálogo do
    ///   tenant — o `price` do `addons_json` (cliente) é IGNORADO (§11).
    async fn verify_item_prices(&self, company_id: Uuid, items: &[OrderItemInput]) -> Result<(), CoreError> {
        // Busca todos os produtos do carrinho numa query (batch — evita
        // N+1 no checkout, §13).
        let ids: Vec<Uuid> = items.iter().map(|i| i.product_id).collect();
        let products = self.product_service.find_by_ids(company_id, &ids).await?;
        let by_id: std::collections::HashMap<Uuid, &crate::product::model::Product> =
            products.iter().map(|p| (p.base.id, p)).collect();
        for item in items {
            let product = *by_id.get(&item.product_id).ok_or_else(|| CoreError::Validation(format!(
                "Item references unknown product_id={}", item.product_id
            )))?;
            // Produto sem preço cadastrado não pode ser vendido (§11):
            // `effective_unit_price` cairia em 0,0 e uma venda a preço zero
            // passaria na verificação. Rejeita explicitamente.
            if product.price.is_none() {
                return Err(CoreError::Validation(format!(
                    "Produto '{}' não tem preço cadastrado e não pode ser vendido",
                    product.name
                )));
            }
            let base = crate::discount::effective_unit_price(product, item.quantity);
            let addons_total = self.addons_total(company_id, product, item.addons_json.as_deref()).await?;
            let expected = base + addons_total;
            if (item.unit_price - expected).abs() > dec!(0.01) {
                return Err(CoreError::Validation(format!(
                    "Price mismatch for product '{}': expected {}, got {}",
                    product.name, money::round2(expected), money::round2(item.unit_price)
                )));
            }
        }
        Ok(())
    }

    /// Soma o preço dos adicionais/opções de variação escolhidos.
    ///
    /// Regras (AI_RULES.md §11): com `addon_service` (servidor), monta o
    /// conjunto de preços LEGÍTIMOS do produto (adicionais dos grupos +
    /// opções de variação, ambos do catálogo do tenant) e valida cada
    /// entrada do `addons_json` por NOME contra esse conjunto, usando o
    /// preço persistido — uma entrada com nome inexistente ou preço
    /// divergente é rejeitada (impede forjar preço de adicional/variação,
    /// já que opções de variação não têm id estável). Sem o service (PDV
    /// desktop, operador confiável), usa o snapshot do cliente.
    async fn addons_total(
        &self,
        company_id: Uuid,
        product: &crate::product::model::Product,
        addons_json: Option<&str>,
    ) -> Result<Decimal, CoreError> {
        let Some(svc) = self.addon_service.as_ref() else {
            return Ok(parse_addons_total(addons_json));
        };
        let Some(s) = addons_json else { return Ok(Decimal::ZERO); };
        let trimmed = s.trim();
        if trimmed.is_empty() { return Ok(Decimal::ZERO); }
        let arr: serde_json::Value = serde_json::from_str(trimmed)
            .map_err(|_| CoreError::Validation("addons_json inválido".into()))?;
        let Some(arr) = arr.as_array() else { return Ok(Decimal::ZERO); };

        // Preços legítimos do produto, por nome (adicionais + variações).
        let mut legit: std::collections::HashMap<String, Decimal> = std::collections::HashMap::new();
        for gid in &product.addon_group_ids {
            for addon in svc.find_by_group(company_id, *gid).await? {
                legit.insert(addon.name, addon.price);
            }
        }
        if let Some(vs) = product.variations.as_deref() {
            if let Ok(serde_json::Value::Array(groups)) =
                serde_json::from_str::<serde_json::Value>(vs)
            {
                for g in &groups {
                    let Some(opts) = g.get("options").and_then(|o| o.as_array()) else { continue };
                    for o in opts {
                        if let (Some(name), Some(price)) = (
                            o.get("name").and_then(|n| n.as_str()),
                            o.get("price").and_then(|p| p.as_f64()).and_then(Decimal::from_f64),
                        ) {
                            legit.insert(name.to_string(), price);
                        }
                    }
                }
            }
        }

        let mut total = Decimal::ZERO;
        for v in arr {
            let name = v
                .get("name")
                .and_then(|n| n.as_str())
                .ok_or_else(|| CoreError::Validation("Adicional sem nome".into()))?;
            let claimed = v.get("price").and_then(|p| p.as_f64()).and_then(Decimal::from_f64);
            let expected = legit.get(name).copied().ok_or_else(|| {
                CoreError::Validation(format!("Adicional '{name}' não pertence a este produto"))
            })?;
            if let Some(c) = claimed {
                if (c - expected).abs() > dec!(0.01) {
                    return Err(CoreError::Validation(format!(
                        "Preço de adicional '{name}' divergente: esperado {}, recebido {}",
                        money::round2(expected), money::round2(c)
                    )));
                }
            }
            total += expected; // usa o preço do catálogo, não o do cliente
        }
        Ok(total)
    }
}

/// Soma `price` dos adicionais do snapshot do cliente (`addons_json`).
/// Usado apenas no caminho confiável (PDV desktop, sem `addon_service`);
/// no servidor o preço é resolvido pelo catálogo (vide `addons_total`).
fn parse_addons_total(addons_json: Option<&str>) -> Decimal {
    let Some(s) = addons_json else { return Decimal::ZERO; };
    let trimmed = s.trim();
    if trimmed.is_empty() { return Decimal::ZERO; }
    let Ok(arr) = serde_json::from_str::<serde_json::Value>(trimmed) else { return Decimal::ZERO; };
    let Some(arr) = arr.as_array() else { return Decimal::ZERO; };
    arr.iter()
        .filter_map(|v| v.get("price").and_then(|p| p.as_f64()).and_then(Decimal::from_f64))
        .sum()
}

/// Valida que a lista de itens não está vazia e cada item é válido.
fn validate_items(items: &[OrderItemInput]) -> Result<(), CoreError> {
    if items.is_empty() {
        return Err(CoreError::Validation("Order must have at least one item".into()));
    }
    for item in items {
        if item.quantity <= 0.0 {
            return Err(CoreError::Validation("Item quantity must be positive".into()));
        }
        if item.unit_price < Decimal::ZERO {
            return Err(CoreError::Validation("Item price cannot be negative".into()));
        }
        if item.product_name.trim().is_empty() {
            return Err(CoreError::Validation("Item product name is required".into()));
        }
    }
    Ok(())
}

/// Detecta se a mensagem de erro do repositório indica violação de UNIQUE INDEX.
/// Cobre tanto PostgreSQL ("duplicate key") quanto SQLite ("UNIQUE constraint").
fn is_unique_violation(msg: &str) -> bool {
    msg.contains("duplicate key") || msg.contains("UNIQUE constraint")
}

/// Constrói OrderItems e calcula o total do pedido.
/// Desconto e total finais do pedido (§11 — puro/testável). O desconto é
/// clampado a `[0, items_total]` (nunca negativo nem maior que os itens); o
/// adicional (acréscimo) é não-negativo e soma; o total nunca fica negativo.
pub fn order_total(
    items_total: Decimal,
    discount_amount: Decimal,
    additional_amount: Decimal,
) -> (Decimal, Decimal) {
    let discount = discount_amount.max(Decimal::ZERO).min(items_total);
    let additional = additional_amount.max(Decimal::ZERO);
    let total = (items_total - discount + additional).max(Decimal::ZERO);
    (discount, total)
}

fn build_items(company_id: Uuid, order_id: Uuid, inputs: &[OrderItemInput]) -> (Vec<OrderItem>, Decimal) {
    let mut total = Decimal::ZERO;
    let items: Vec<OrderItem> = inputs.iter().map(|i| {
        let item = OrderItem::new(
            company_id,
            order_id,
            i.product_id,
            i.product_name.clone(),
            i.quantity,
            i.unit_price,
            i.notes.clone(),
            i.addons_json.clone(),
        );
        total += item.subtotal;
        item
    }).collect();
    (items, total)
}
