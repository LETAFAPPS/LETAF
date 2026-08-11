use std::sync::Arc;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use uuid::Uuid;

use crate::money;

use super::model::{DeliveryType, Order, OrderItem, OrderStatus};
use super::repository::{CouponLimits, OrderRepository};
use crate::addon::service::AddonService;
use crate::cash::service::CashService;
use crate::error::CoreError;
use crate::product::service::ProductService;

/// Teto de sanidade para a quantidade de um item. Acima disso o subtotal
/// (`qty * unit_price`) poderia estourar `NUMERIC(14,2)` no Postgres — o guard
/// devolve erro de validação limpo em vez de um 500 cru. Um milhão de unidades
/// já é ordens de grandeza acima de qualquer venda real (§13).
const MAX_ITEM_QUANTITY: f64 = 1_000_000.0;

/// Teto de sanidade para o NÚMERO de itens de um pedido. Sem ele, o único
/// limite era o corpo global de 10 MB (~110 mil itens), e cada item dispara
/// trabalho O(n) (verificação de preço, adicionais, delta de estoque) — um
/// cliente autenticado poderia forjar um pedido gigante e prender um worker
/// e uma conexão do pool por minutos. Nenhum pedido real de food service ou
/// PDV passa disso; acima → erro de validação limpo (§11/§13). `pub` para
/// que a rota web (não-confiável) barre o tamanho ANTES dos laços caros —
/// aqui no core é a defesa em profundidade final.
pub const MAX_ORDER_ITEMS: usize = 200;

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
    /// Opcional (servidor): grupos de adicionais, para resolver preço por
    /// grupo e validar os limites de seleção.
    addon_group_service: Option<Arc<crate::addon_group::service::AddonGroupService>>,
}

impl OrderService {
    pub fn new(repo: Arc<dyn OrderRepository>, product_service: Arc<ProductService>) -> Self {
        Self {
            repo,
            product_service,
            cash_service: None,
            addon_service: None,
            addon_group_service: None,
        }
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

    /// Builder fluente para injetar o `AddonGroupService` (servidor).
    ///
    /// Necessário para resolver o preço DENTRO do grupo declarado e para
    /// validar `min_select`/`max_select`, que antes só existiam no
    /// cliente (§11).
    pub fn with_addon_group_service(mut self, groups: Arc<crate::addon_group::service::AddonGroupService>) -> Self {
        self.addon_group_service = Some(groups);
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
        // Taxa de entrega, resolvida pelo CALLER a partir do cadastro da
        // empresa (§11 — nunca vem da requisição). O PDV já cobrava; o
        // checkout do cardápio web saía sem a taxa, e a loja perdia o
        // frete de todo pedido online.
        additional_amount: Decimal,
        // Limites do cupom para enforcement race-safe DENTRO da transação
        // (§11). `None` = sem cupom. O desconto já vem calculado acima; isto
        // só serializa/reconta o USO para fechar o TOCTOU do limite.
        coupon_limits: Option<CouponLimits>,
    ) -> Result<Order, CoreError> {
        validate_items(&items)?;
        let list_prices = self.verify_item_prices(company_id, &items).await?;

        let order_id = Uuid::new_v4();
        let (final_items, items_total) = build_items(company_id, order_id, &items, &list_prices);
        // Desconto vem JÁ calculado/validado pelo caller (server),
        // que recomputa via CouponService — nunca confiamos no valor
        // do frontend (§11). Aqui só garantimos que não fica negativo.
        let (discount, total) = order_total(items_total, discount_amount, additional_amount);
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
            order.additional_amount = additional_amount;
            order.number = self.repo.next_number(company_id).await?;
            order.items = final_items.clone();
            // Colisão de número sequencial reverte a tx (estoque incluso)
            // e tenta de novo — sem rollback manual de estoque.
            match self.repo.create_atomic(&order, &stock_deltas, coupon_limits.as_ref()).await {
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
                    "Forma de pagamento desconhecida: '{method}'"
                )));
            }
        }
        let list_prices = self.verify_item_prices(company_id, &items).await?;

        let order_id = Uuid::new_v4();
        let (final_items, items_total) = build_items(company_id, order_id, &items, &list_prices);
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
            // Venda de PDV com forma de pagamento = paga na finalização.
            order.paid = payment_method.is_some();
            order.discount_amount = discount;
            order.additional_amount = additional;
            order.number = self.repo.next_number(company_id).await?;
            order.items = final_items.clone();
            // PDV não aplica limite de cupom web (desconto do PDV é manual/
            // offline); sem enforcement de uso → `None`.
            match self.repo.create_atomic(&order, &stock_deltas, None).await {
                Ok(()) => {
                    // Lança movimento na sessão de caixa, se houver.
                    // Falha NÃO desfaz a venda — pedido é fonte de
                    // verdade; movimento é só reflexo agregado.
                    //
                    // `payment_method == "wallet"` entra como venda da
                    // sessão (aparece no resumo por forma de pagamento),
                    // mas NÃO altera o saldo da gaveta: o dinheiro já
                    // entrou no depósito do cliente. O débito do saldo
                    // vira `WalletMovement::OrderCharge`, disparado pelo
                    // controller PDV após este retorno.
                    if let (Some(cash), Some(sid), Some(method)) =
                        (&self.cash_service, session_id, order.payment_method.as_ref())
                    {
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

    /// Total de registros ativos da empresa (painel do super admin).
    pub async fn count_all(&self, company_id: Uuid) -> Result<i64, CoreError> {
        self.repo.count_all(company_id).await
    }

    /// Pedidos do período (datas inclusivas) — insumo dos relatórios.
    pub async fn find_in_period(
        &self,
        company_id: Uuid,
        inicio: chrono::NaiveDate,
        fim: chrono::NaiveDate,
    ) -> Result<Vec<Order>, CoreError> {
        self.repo.find_in_period(company_id, inicio, fim).await
    }

    /// Fiados em aberto, de qualquer data (o "a receber" não tem período).
    pub async fn find_unpaid_wallet(&self, company_id: Uuid) -> Result<Vec<Order>, CoreError> {
        self.repo.find_unpaid_wallet(company_id).await
    }

    /// Busca paginada por número ou cliente, em todo o histórico.
    ///
    /// Termo vazio devolve lista vazia: quem não filtrou quer o quadro, não
    /// o histórico inteiro.
    pub async fn search(
        &self,
        company_id: Uuid,
        termo: &str,
        limite: i64,
        deslocamento: i64,
    ) -> Result<Vec<Order>, CoreError> {
        let termo = termo.trim();
        if termo.is_empty() {
            return Ok(Vec::new());
        }
        self.repo.search(company_id, termo, limite, deslocamento).await
    }

    /// Pedidos do quadro operacional (ver o trait).
    pub async fn find_board(
        &self,
        company_id: Uuid,
        entregues_desde: chrono::NaiveDate,
    ) -> Result<Vec<Order>, CoreError> {
        self.repo.find_board(company_id, entregues_desde).await
    }

    /// Pedidos sem os itens — para telas que só listam (ver o trait).
    pub async fn find_all_light(&self, company_id: Uuid) -> Result<Vec<Order>, CoreError> {
        self.repo.find_all_light(company_id).await
    }

    pub async fn find_all(&self, company_id: Uuid) -> Result<Vec<Order>, CoreError> {
        self.repo.find_all(company_id).await
    }

    /// Página da listagem de operador (mais recentes primeiro).
    pub async fn find_all_paged(
        &self,
        company_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Order>, CoreError> {
        self.repo.find_all_paged(company_id, limit, offset).await
    }

    /// Pedidos em andamento (badge da navegação).
    pub async fn count_active(&self, company_id: Uuid) -> Result<i64, CoreError> {
        self.repo.count_active(company_id).await
    }

    pub async fn find_by_customer(&self, company_id: Uuid, customer_id: Uuid) -> Result<Vec<Order>, CoreError> {
        self.repo.find_by_customer(company_id, customer_id).await
    }

    /// Conta usos de um cupom (não-cancelados) por query dedicada —
    /// evita carregar todos os pedidos no checkout (§13).
    pub async fn count_coupon_uses(&self, company_id: Uuid, coupon_code: &str) -> Result<i64, CoreError> {
        self.repo.count_coupon_uses(company_id, coupon_code).await
    }

    pub async fn count_customer_orders(&self, company_id: Uuid, customer_id: Uuid) -> Result<i64, CoreError> {
        self.repo.count_customer_orders(company_id, customer_id).await
    }

    pub async fn count_customer_coupon_uses(
        &self,
        company_id: Uuid,
        customer_id: Uuid,
        coupon_code: &str,
    ) -> Result<i64, CoreError> {
        self.repo
            .count_customer_coupon_uses(company_id, customer_id, coupon_code)
            .await
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

    /// Marca como PAGOS todos os pedidos fiados (carteira, não pagos)
    /// do cliente — chamado quando a dívida da carteira zera (o fiado
    /// foi quitado). Cancelados ficam de fora.
    pub async fn settle_wallet_orders(
        &self,
        company_id: Uuid,
        customer_id: Uuid,
    ) -> Result<u32, CoreError> {
        // Por CLIENTE (há índice `idx_orders_customer`) em vez de
        // `find_all`, que trazia todos os pedidos da empresa — com os
        // itens — só para filtrar os de um cliente. §13.
        let orders = self.repo.find_by_customer(company_id, customer_id).await?;
        let mut settled = 0u32;
        for o in orders.iter().filter(|o| {
            o.customer_id == customer_id
                && o.payment_method.as_deref() == Some("wallet")
                && !o.paid
                && o.status != OrderStatus::Cancelled
        }) {
            self.repo
                .update_payment(company_id, o.base.id, Some("wallet"), true)
                .await?;
            settled += 1;
        }
        Ok(settled)
    }

    /// Registra a forma de pagamento e o status Pago/Não pago do pedido.
    ///
    /// Regras (AI_RULES.md §1, §7, §11):
    /// - Backend valida a forma contra [`PAYMENT_METHODS`] — nunca
    ///   confia no rótulo vindo da UI.
    /// - Pedido cancelado não recebe pagamento.
    /// - Update dedicado (marca `synced = false`) → sync propaga (§7).
    pub async fn set_payment(
        &self,
        company_id: Uuid,
        id: Uuid,
        payment_method: Option<String>,
        paid: bool,
    ) -> Result<Order, CoreError> {
        if let Some(method) = payment_method.as_deref() {
            if !crate::order::model::PAYMENT_METHODS.contains(&method) {
                return Err(CoreError::Validation(format!(
                    "Forma de pagamento desconhecida: '{method}'"
                )));
            }
        }
        let mut order = self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("Order not found".into()))?;
        if order.status == OrderStatus::Cancelled {
            return Err(CoreError::Validation(
                "Pedido cancelado não recebe pagamento".into(),
            ));
        }
        self.repo
            .update_payment(company_id, id, payment_method.as_deref(), paid)
            .await?;
        order.payment_method = payment_method;
        order.paid = paid;
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
        // Pedido PAGO é FECHADO: editar itens mudaria o total sem reconciliar o
        // caixa/carteira já registrados na venda, gerando quebra no fechamento.
        // Regra: bloquear — para alterar, cancele e refaça (§11 — o backend é a
        // autoridade; a UI só reflete). O `cancel` estorna o caixa; o novo
        // pedido registra a venda correta.
        if order.paid {
            return Err(CoreError::Validation(
                "Pedido pago não pode ser editado. Cancele e refaça o pedido.".into(),
            ));
        }
        if new_items.is_empty() {
            return Err(CoreError::Validation(
                "Pedido precisa ter ao menos um item".into(),
            ));
        }
        // §11/§13: mesmo teto do checkout na EDIÇÃO — antes do `verify_item_prices`
        // (que faz trabalho O(n) por item), para não amplificar via edição.
        if new_items.len() > MAX_ORDER_ITEMS {
            return Err(CoreError::Validation(
                "O pedido excede o número máximo de itens".into(),
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
        let list_prices = self.verify_item_prices(company_id, &price_check).await?;

        let now = chrono::Utc::now().naive_utc();
        let mut finalized: Vec<super::model::OrderItem> = Vec::with_capacity(new_items.len());
        for (idx, mut it) in new_items.into_iter().enumerate() {
            if !it.quantity.is_finite() || it.quantity <= 0.0 {
                return Err(CoreError::Validation(
                    "Quantidade de item deve ser positiva".into(),
                ));
            }
            if it.quantity > MAX_ITEM_QUANTITY {
                return Err(CoreError::Validation(
                    "Quantidade de item excede o máximo permitido".into(),
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
            // Snapshot do preço de tabela recalculado pelo catálogo atual
            // (mesma ordem do price_check → índice bate com new_items).
            it.list_unit_price = list_prices.get(idx).copied();
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

        // Edição + ajuste de estoque na MESMA transação (§4, §7.6): sem janela
        // de divergência pedido×estoque. delta = qty ANTIGA − qty NOVA (positivo
        // restitui, negativo baixa). Insuficiente aborta a edição inteira.
        let deltas: Vec<(Uuid, f64)> = stock_delta.into_iter().collect();
        self.repo.update_atomic(&order, &deltas).await?;
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
                "Informe o motivo do cancelamento".into(),
            ));
        }
        let order = self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("Order not found".into()))?;
        match order.status {
            OrderStatus::Delivered => {
                return Err(CoreError::Validation(
                    "Pedidos entregues não podem ser cancelados".into(),
                ));
            }
            OrderStatus::Cancelled => {
                return Err(CoreError::Validation("Este pedido já foi cancelado".into()));
            }
            _ => {}
        }
        // Cancela E restitui o estoque na MESMA transação (§4, §7.6): sem
        // janela de estoque-fantasma se o processo cair no meio. Produtos
        // ilimitados/excluídos são pulados pelo repo (o cancel não falha por
        // estoque). O ledger idempotente propaga a devolução via sync.
        // AGREGA por produto: o pedido pode ter duas linhas do mesmo
        // item, e o id do movimento de cancelamento é derivado de
        // (pedido, produto) — duas linhas separadas colapsariam no mesmo
        // id e uma das devoluções seria perdida.
        let mut por_produto: std::collections::HashMap<Uuid, f64> =
            std::collections::HashMap::new();
        for item in &order.items {
            *por_produto.entry(item.product_id).or_insert(0.0) += item.quantity;
        }
        let restitutions: Vec<(Uuid, f64)> = por_produto.into_iter().collect();
        self.repo.cancel_atomic(company_id, id, trimmed, &restitutions).await?;

        // Estorna a VENDA na sessão de caixa: sem isso a gaveta seguia
        // esperando o dinheiro de um pedido cancelado e o fechamento
        // acusava quebra. Falha aqui não desfaz o cancelamento (o pedido
        // é a fonte de verdade) — só loga, igual às demais pontes.
        if let (Some(cash), Some(method)) =
            (&self.cash_service, order.payment_method.as_deref())
        {
            if let Err(e) = cash
                .register_sale_reversal(
                    company_id,
                    order.base.id,
                    order.total,
                    method.to_string(),
                )
                .await
            {
                tracing::warn!("pedido {} cancelado, mas o caixa não estornou: {e}", order.base.id);
            }
        }

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

    /// Página do pull por keyset `(updated_at, id)`.
    pub async fn find_updated_since_paged(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
        after_id: Uuid,
        limit: i64,
    ) -> Result<Vec<Order>, CoreError> {
        self.repo.find_updated_since_paged(company_id, since, after_id, limit).await
    }

    /// Upsert do sync. `false` = a versão recebida foi DESCARTADA por
    /// existir uma mais nova (ver `OrderRepository::sync_upsert`).
    pub async fn sync_upsert(&self, company_id: Uuid, mut order: Order) -> Result<bool, CoreError> {
        if order.base.company_id != company_id {
            return Err(CoreError::Validation("Operação não permitida para esta empresa".into()));
        }
        order.base.clamp_future_updated_at();
        order.base.synced = true;
        for item in &mut order.items {
            // Normaliza o filho ao tenant e ao pai — NÃO confia no payload
            // (§11). `OrderItem` tem `company_id` e `order_id` livres no JSON;
            // sem isto, um operador de uma loja forjava um item com o
            // `company_id`/`order_id` de OUTRA empresa e o upsert o gravava na
            // partição dela (a guarda do `ON CONFLICT` comparava payload
            // contra payload, não contra o tenant do JWT).
            item.base.company_id = company_id;
            item.order_id = order.base.id;
            item.base.synced = true;
        }
        // §11: subtotal, desconto e total são autoridade do SERVIDOR também no
        // sync — recomputados a partir de `qty × unit_price` (mesma fórmula de
        // `create`/`update_basics`). Fecha o vetor de `total`/`subtotal` forjado
        // no sync (inflar/deflacionar receita, que contaminava tesouraria e
        // relatórios). O `unit_price` continua snapshot offline — não é
        // reconferido contra o catálogo (inerente ao LWW).
        normalize_order_money(&mut order);
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
    ///
    /// Devolve, na ordem dos `items`, o preço unitário DE TABELA de cada
    /// um (preço do produto + adicionais, SEM o desconto do produto) —
    /// snapshot gravado no `OrderItem.list_unit_price` para a UI exibir
    /// o desconto depois (recibo/detalhe) sem reconsultar o catálogo.
    async fn verify_item_prices(&self, company_id: Uuid, items: &[OrderItemInput]) -> Result<Vec<Decimal>, CoreError> {
        // Busca todos os produtos do carrinho numa query (batch — evita
        // N+1 no checkout, §13).
        let ids: Vec<Uuid> = items.iter().map(|i| i.product_id).collect();
        let products = self.product_service.find_by_ids(company_id, &ids).await?;
        let by_id: std::collections::HashMap<Uuid, &crate::product::model::Product> =
            products.iter().map(|p| (p.base.id, p)).collect();
        let mut list_prices: Vec<Decimal> = Vec::with_capacity(items.len());
        for item in items {
            let product = *by_id.get(&item.product_id).ok_or_else(|| CoreError::Validation(format!(
                "Item referencia produto inexistente (product_id={})", item.product_id
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
            list_prices.push(product.price.unwrap_or(Decimal::ZERO) + addons_total);
            if (item.unit_price - expected).abs() > dec!(0.01) {
                return Err(CoreError::Validation(format!(
                    "Preço divergente para o produto '{}': esperado {}, recebido {}",
                    product.name, money::round2(expected), money::round2(item.unit_price)
                )));
            }
            // Regras de composição da variação (§11) — só no servidor
            // (`addon_service` presente); o PDV desktop é operador confiável,
            // igual à resolução de preço de adicional acima.
            if self.addon_service.is_some() {
                validate_required_variations(
                    product.variations.as_deref(),
                    &product.name,
                    item.addons_json.as_deref(),
                )?;
            }
        }
        Ok(list_prices)
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

        // Preços legítimos por nome. Um mesmo nome pode existir em grupos
        // distintos com preços distintos (ex.: "Grande" em Tamanho e em Copo),
        // então guardamos TODOS os preços válidos daquele nome — sem "o último
        // sobrescreve" (que causava preço errado / rejeição de seleção válida).
        let mut legit: std::collections::HashMap<String, Vec<Decimal>> = std::collections::HashMap::new();
        // Por GRUPO também: o snapshot carrega o grupo de origem e o
        // preço é resolvido DENTRO dele. Sem isso, "Grande" do grupo
        // Refrigerante (R$ 5) era aceito como preço de "Grande" da
        // variação Tamanho (R$ 20) — dava para pagar a opção cara pelo
        // preço da barata (§11).
        let mut por_grupo: std::collections::HashMap<(String, String), Vec<Decimal>> =
            std::collections::HashMap::new();
        // Quantos itens de cada grupo o cliente selecionou, para conferir
        // `min_select`/`max_select` — que só existiam no cliente.
        let mut grupos: Vec<crate::addon_group::model::AddonGroup> = Vec::new();
        for gid in &product.addon_group_ids {
            let grupo = match self.addon_group_service.as_ref() {
                Some(gs) => gs.find_by_id(company_id, *gid).await?,
                None => None,
            };
            if let Some(g) = grupo {
                for addon in svc.find_by_group(company_id, *gid).await? {
                    legit.entry(addon.name.clone()).or_default().push(addon.price);
                    por_grupo
                        .entry((g.name.clone(), addon.name))
                        .or_default()
                        .push(addon.price);
                }
                grupos.push(g);
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
                            o.get("price").and_then(money::price_from_json),
                        ) {
                            legit.entry(name.to_string()).or_default().push(price);
                            if let Some(titulo) = g.get("title").and_then(|t| t.as_str()) {
                                por_grupo
                                    .entry((titulo.to_string(), name.to_string()))
                                    .or_default()
                                    .push(price);
                            }
                        }
                    }
                }
            }
        }

        let mut total = Decimal::ZERO;
        let mut por_grupo_selecionados: std::collections::HashMap<String, i32> =
            std::collections::HashMap::new();
        for v in arr {
            let name = v
                .get("name")
                .and_then(|n| n.as_str())
                .ok_or_else(|| CoreError::Validation("Adicional sem nome".into()))?;
            let grupo = v.get("group").and_then(|g| g.as_str()).unwrap_or("");
            if !grupo.is_empty() {
                *por_grupo_selecionados.entry(grupo.to_string()).or_insert(0) += 1;
            }
            // Com o grupo declarado, os preços legítimos são só os DAQUELE
            // grupo. Sem ele (snapshot legado), cai no conjunto por nome.
            let candidates = por_grupo
                .get(&(grupo.to_string(), name.to_string()))
                .filter(|c| !c.is_empty())
                .or_else(|| legit.get(name).filter(|c| !c.is_empty()))
                .ok_or_else(|| {
                    CoreError::Validation(format!(
                        "Adicional '{name}' não pertence a este produto"
                    ))
                })?;
            let claimed = v.get("price").and_then(money::price_from_json);
            // Usa o preço do CATÁLOGO (nunca o do cliente). Com preço
            // reivindicado, exige que case com algum preço legítimo do nome;
            // sem ele, usa o primeiro (compat. com snapshot legado sem preço).
            let chosen = match claimed {
                Some(c) => *candidates
                    .iter()
                    .find(|&&p| (p - c).abs() <= dec!(0.01))
                    .ok_or_else(|| CoreError::Validation(format!(
                        "Preço de adicional '{name}' divergente do catálogo"
                    )))?,
                None => candidates[0],
            };
            total += chosen;
        }

        // §11: `min_select`/`max_select` viviam só no cliente — uma
        // requisição forjada ignorava grupo obrigatório e teto de escolha.
        for g in &grupos {
            let n = por_grupo_selecionados.get(&g.name).copied().unwrap_or(0);
            // Sem nenhum item daquele grupo no snapshot pode ser um
            // snapshot legado (sem `group`): só cobra o mínimo quando o
            // cliente declarou os grupos.
            let declarou_grupos = !por_grupo_selecionados.is_empty();
            if declarou_grupos && g.min_select > 0 && n < g.min_select {
                return Err(CoreError::Validation(format!(
                    "Escolha ao menos {} opção(ões) em '{}'",
                    g.min_select, g.name
                )));
            }
            let teto = if g.selection == "single" { 1 } else { g.max_select };
            if teto > 0 && n > teto {
                return Err(CoreError::Validation(format!(
                    "No máximo {} opção(ões) em '{}'",
                    teto, g.name
                )));
            }
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
        .filter_map(|v| v.get("price").and_then(money::price_from_json))
        .sum()
}

/// Valida (servidor) que toda variação `required` do produto tem ao menos uma
/// seleção no `addons_json`. CONSERVADOR (§11): só rejeita quando é possível
/// afirmar sem ambiguidade que um grupo obrigatório ficou sem seleção. Uma
/// seleção é atribuída ao grupo por `group`==título OU por `name` ∈ opções do
/// grupo (mesma base por-nome que a verificação de preço já usa). Casos
/// ambíguos (JSON malformado, grupo obrigatório sem opções) → aceita.
fn validate_required_variations(
    variations: Option<&str>,
    product_name: &str,
    addons_json: Option<&str>,
) -> Result<(), CoreError> {
    let Some(vs) = variations else { return Ok(()); };
    let Ok(serde_json::Value::Array(groups)) =
        serde_json::from_str::<serde_json::Value>(vs)
    else {
        return Ok(()); // JSON inválido: não é papel deste check punir.
    };

    // Seleções do cliente: (grupo opcional, nome).
    let selections: Vec<(Option<String>, String)> = addons_json
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|sel| {
            let name = sel.get("name").and_then(|n| n.as_str())?.to_string();
            let group = sel.get("group").and_then(|g| g.as_str()).map(String::from);
            Some((group, name))
        })
        .collect();

    for g in &groups {
        if !g.get("required").and_then(|r| r.as_bool()).unwrap_or(false) {
            continue;
        }
        let Some(title) = g.get("title").and_then(|t| t.as_str()) else { continue };
        let opt_names: std::collections::HashSet<&str> = g
            .get("options")
            .and_then(|o| o.as_array())
            .map(|opts| opts.iter().filter_map(|o| o.get("name").and_then(|n| n.as_str())).collect())
            .unwrap_or_default();
        // Grupo obrigatório sem opções conhecidas: ambíguo → aceita.
        if opt_names.is_empty() {
            continue;
        }
        let satisfied = selections.iter().any(|(sel_group, sel_name)| {
            sel_group.as_deref() == Some(title) || opt_names.contains(sel_name.as_str())
        });
        if !satisfied {
            return Err(CoreError::Validation(format!(
                "Selecione a opção obrigatória de '{title}' em '{product_name}'"
            )));
        }
    }
    Ok(())
}

/// Valida que a lista de itens não está vazia e cada item é válido.
fn validate_items(items: &[OrderItemInput]) -> Result<(), CoreError> {
    if items.is_empty() {
        return Err(CoreError::Validation("O pedido deve ter ao menos um item".into()));
    }
    // Teto no número de itens: barra na raiz o vetor de exaustão (item mínimo
    // ~90 bytes cabe dezenas de milhares no corpo de 10 MB) que amplificaria
    // todos os laços por-item do checkout. §11/§13.
    if items.len() > MAX_ORDER_ITEMS {
        return Err(CoreError::Validation("O pedido excede o número máximo de itens".into()));
    }
    for item in items {
        if !item.quantity.is_finite() || item.quantity <= 0.0 {
            return Err(CoreError::Validation("A quantidade do item deve ser positiva".into()));
        }
        // Teto de sanidade: acima disso o subtotal estouraria NUMERIC(14,2) no
        // Postgres (erro 500 cru). Recusa com validação limpa. §13.
        if item.quantity > MAX_ITEM_QUANTITY {
            return Err(CoreError::Validation("Quantidade de item excede o máximo permitido".into()));
        }
        if item.unit_price < Decimal::ZERO {
            return Err(CoreError::Validation("O preço do item não pode ser negativo".into()));
        }
        if item.product_name.trim().is_empty() {
            return Err(CoreError::Validation("Informe o nome do produto do item".into()));
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

/// Recomputa a aritmética de dinheiro de um pedido a partir de `qty × unit_price`
/// — autoridade do servidor no caminho de sync (§11 — puro/testável). Sobrescreve
/// cada `subtotal`, o `discount_amount` (clampado) e o `total`. NÃO reconfere o
/// `unit_price` contra o catálogo (snapshot offline, inerente ao LWW).
fn normalize_order_money(order: &mut Order) {
    for item in &mut order.items {
        item.subtotal = money::round2(money::qty(item.quantity) * item.unit_price);
    }
    let items_total: Decimal = order.items.iter().map(|i| i.subtotal).sum();
    let (discount, total) =
        order_total(items_total, order.discount_amount, order.additional_amount);
    order.discount_amount = discount;
    order.total = total;
}

/// `list_prices` vem de `verify_item_prices` (mesma ordem dos inputs):
/// preço de tabela por unidade, gravado como snapshot no item para a UI
/// exibir o desconto do produto depois da venda.
fn build_items(
    company_id: Uuid,
    order_id: Uuid,
    inputs: &[OrderItemInput],
    list_prices: &[Decimal],
) -> (Vec<OrderItem>, Decimal) {
    let mut total = Decimal::ZERO;
    let items: Vec<OrderItem> = inputs.iter().enumerate().map(|(idx, i)| {
        let mut item = OrderItem::new(
            company_id,
            order_id,
            i.product_id,
            i.product_name.clone(),
            i.quantity,
            i.unit_price,
            i.notes.clone(),
            i.addons_json.clone(),
        );
        item.list_unit_price = list_prices.get(idx).copied();
        total += item.subtotal;
        item
    }).collect();
    (items, total)
}

#[cfg(test)]
mod tests {
    use super::{validate_items, validate_required_variations, OrderItemInput, MAX_ORDER_ITEMS};
    use rust_decimal::Decimal;
    use uuid::Uuid;

    fn item_min() -> OrderItemInput {
        OrderItemInput {
            product_id: Uuid::nil(),
            product_name: "x".into(),
            quantity: 1.0,
            unit_price: Decimal::ONE,
            notes: None,
            addons_json: None,
        }
    }

    #[test]
    fn rejeita_pedido_acima_do_teto_de_itens() {
        // No teto: OK.
        let ok: Vec<_> = (0..MAX_ORDER_ITEMS).map(|_| item_min()).collect();
        assert!(validate_items(&ok).is_ok());
        // Acima do teto: erro de validação limpo (não 500 / não worker preso).
        let over: Vec<_> = (0..=MAX_ORDER_ITEMS).map(|_| item_min()).collect();
        assert!(validate_items(&over).is_err());
    }

    #[test]
    fn normalize_order_money_neutraliza_total_e_subtotal_forjados() {
        use super::normalize_order_money;
        use crate::order::model::{DeliveryType, Order, OrderItem};
        use rust_decimal_macros::dec;

        let cid = Uuid::nil();
        // Total forjado gigante no payload.
        let mut order = Order::new(cid, Uuid::nil(), dec!(999999.00), DeliveryType::Pickup, None);
        // Item legítimo (2 × 10,00 = 20,00), mas com subtotal forjado.
        let mut it =
            OrderItem::new(cid, Uuid::nil(), Uuid::nil(), "x".into(), 2.0, dec!(10.00), None, None);
        it.subtotal = dec!(1.00);
        order.items.push(it);
        order.discount_amount = dec!(5.00);
        order.additional_amount = dec!(3.00); // ex.: taxa de entrega

        normalize_order_money(&mut order);

        // Subtotal recomputado de qty×preço; total = 20 − 5 + 3 = 18.
        assert_eq!(order.items[0].subtotal, dec!(20.00));
        assert_eq!(order.total, dec!(18.00));
        assert_eq!(order.discount_amount, dec!(5.00));
    }

    #[test]
    fn normalize_order_money_clampa_desconto_ao_total_dos_itens() {
        use super::normalize_order_money;
        use crate::order::model::{DeliveryType, Order, OrderItem};
        use rust_decimal_macros::dec;

        let cid = Uuid::nil();
        let mut order = Order::new(cid, Uuid::nil(), dec!(0.00), DeliveryType::Pickup, None);
        order.items.push(OrderItem::new(
            cid, Uuid::nil(), Uuid::nil(), "x".into(), 1.0, dec!(10.00), None, None,
        ));
        // Desconto absurdo (> itens) é clampado ao total dos itens → total 0.
        order.discount_amount = dec!(999.00);
        normalize_order_money(&mut order);
        assert_eq!(order.discount_amount, dec!(10.00));
        assert_eq!(order.total, dec!(0.00));
    }


    // Produto com uma variação OBRIGATÓRIA "Sabor" (opções Calabresa/Frango)
    // e uma NÃO-obrigatória "Borda".
    const VARIATIONS: &str = r#"[
        {"title":"Sabor","required":true,"options":[{"name":"Calabresa","price":"0.00"},{"name":"Frango","price":"2.00"}]},
        {"title":"Borda","required":false,"options":[{"name":"Cheddar","price":"3.00"}]}
    ]"#;

    #[test]
    fn rejeita_quando_obrigatoria_sem_selecao() {
        // Sem addons_json → variação obrigatória não atendida.
        assert!(validate_required_variations(Some(VARIATIONS), "Pizza", None).is_err());
        // addons_json só com a borda (não-obrigatória) → falta o Sabor.
        let only_borda = r#"[{"group":"Borda","name":"Cheddar","price":"3.00"}]"#;
        assert!(validate_required_variations(Some(VARIATIONS), "Pizza", Some(only_borda)).is_err());
    }

    #[test]
    fn aceita_com_selecao_por_grupo_ou_por_nome() {
        // Schema novo (com "group").
        let by_group = r#"[{"group":"Sabor","name":"Calabresa","price":"0.00"}]"#;
        assert!(validate_required_variations(Some(VARIATIONS), "Pizza", Some(by_group)).is_ok());
        // Schema legado (sem "group") → atribui pelo nome da opção.
        let by_name = r#"[{"name":"Frango","price":"2.00"}]"#;
        assert!(validate_required_variations(Some(VARIATIONS), "Pizza", Some(by_name)).is_ok());
    }

    #[test]
    fn aceita_em_casos_ambiguos_ou_sem_obrigatoria() {
        // Produto sem variações → nada a exigir.
        assert!(validate_required_variations(None, "X", None).is_ok());
        // JSON malformado → não pune.
        assert!(validate_required_variations(Some("{lixo"), "X", None).is_ok());
        // Variação obrigatória SEM opções conhecidas → ambíguo → aceita.
        let no_opts = r#"[{"title":"Sabor","required":true,"options":[]}]"#;
        assert!(validate_required_variations(Some(no_opts), "X", None).is_ok());
    }
}
