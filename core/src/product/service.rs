use std::sync::Arc;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use uuid::Uuid;

use super::model::{BalanceMode, Product};
use super::repository::ProductRepository;
use super::stock_movement::StockMovement;
use crate::error::CoreError;

/// Service para o domínio Product.
///
/// Regras aplicadas (AI_RULES.md §1, §9, §11):
/// - service.rs contém a orquestração de regras de negócio
/// - Depende de repository via trait (inversão de dependência)
/// - Validar todos os dados de entrada no backend
///
/// O handler passa dados brutos; o service valida,
/// constrói a entidade e gerencia timestamps/synced.
pub struct ProductService {
    repo: Arc<dyn ProductRepository>,
}

impl ProductService {
    pub fn new(repo: Arc<dyn ProductRepository>) -> Self {
        Self { repo }
    }

    pub async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Product>, CoreError> {
        self.repo.find_by_id(company_id, id).await
    }

    pub async fn find_all(&self, company_id: Uuid) -> Result<Vec<Product>, CoreError> {
        self.repo.find_all(company_id).await
    }

    /// Busca vários produtos por id numa query (batch, evita N+1).
    pub async fn find_by_ids(&self, company_id: Uuid, ids: &[Uuid]) -> Result<Vec<Product>, CoreError> {
        self.repo.find_by_ids(company_id, ids).await
    }

    /// Cria um produto a partir de dados brutos.
    ///
    /// Valida entrada, constrói entidade, persiste e retorna.
    ///
    /// `balance_mode` é relevante quando `unit == "kg"` (define se o EAN-13
    /// emitido pela balança encoda peso ou preço). Para outras unidades,
    /// o valor é persistido mas ignorado pelo PDV.
    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        &self,
        company_id: Uuid,
        name: String,
        description: Option<String>,
        category_id: Option<Uuid>,
        subcategory_id: Option<Uuid>,
        price: Option<Decimal>,
        cost_price: Option<Decimal>,
        stock_quantity: f64,
        min_stock: f64,
        unlimited_stock: bool,
        barcode: Option<String>,
        unit: String,
        balance_mode: BalanceMode,
        image_data: Option<String>,
        cover_color: Option<String>,
        availability_schedule: Option<String>,
        discount_kind: Option<String>,
        discount_value: Option<Decimal>,
        discount_min_qty: Option<f64>,
        discount_tiers: Option<String>,
        addon_group_ids: Vec<Uuid>,
        variations: Option<String>,
    ) -> Result<Product, CoreError> {
        Self::validate_product(&name, price, cost_price, stock_quantity, min_stock, unlimited_stock)?;
        Self::validate_discount(&discount_kind, discount_value, discount_min_qty, &discount_tiers)?;
        Self::validate_variations(&variations)?;
        // Produto ilimitado força stock_quantity = 0 (campo deixa de ser
        // semântico). Mantemos 0 em vez de None para reaproveitar a coluna
        // existente sem nullable cascade.
        let effective_stock = if unlimited_stock { 0.0 } else { stock_quantity };
        let mut product = Product::new(
            company_id, name, description, category_id, subcategory_id, price,
            cost_price, effective_stock, min_stock, unlimited_stock, barcode, unit, balance_mode,
            image_data, cover_color, availability_schedule, discount_kind, discount_value,
            discount_min_qty, discount_tiers,
        );
        product.variations = variations;
        self.repo.create(&product).await?;
        // Persiste as associações N:M depois do produto existir.
        // Validação cross-tenant fica no DB (FK + filtro company_id no
        // repo); aqui passamos a lista direto.
        if !addon_group_ids.is_empty() {
            self.repo.replace_addon_groups(company_id, product.base.id, &addon_group_ids).await?;
        }
        product.addon_group_ids = addon_group_ids;
        Ok(product)
    }

    /// Atualiza um produto existente.
    ///
    /// Busca, valida, aplica alterações, atualiza timestamps e persiste.
    #[allow(clippy::too_many_arguments)]
    pub async fn update(
        &self,
        company_id: Uuid,
        id: Uuid,
        name: String,
        description: Option<String>,
        category_id: Option<Uuid>,
        subcategory_id: Option<Uuid>,
        price: Option<Decimal>,
        cost_price: Option<Decimal>,
        stock_quantity: f64,
        min_stock: f64,
        unlimited_stock: bool,
        barcode: Option<String>,
        unit: String,
        balance_mode: BalanceMode,
        image_data: Option<String>,
        cover_color: Option<String>,
        availability_schedule: Option<String>,
        discount_kind: Option<String>,
        discount_value: Option<Decimal>,
        discount_min_qty: Option<f64>,
        discount_tiers: Option<String>,
        addon_group_ids: Vec<Uuid>,
        variations: Option<String>,
    ) -> Result<Product, CoreError> {
        Self::validate_product(&name, price, cost_price, stock_quantity, min_stock, unlimited_stock)?;
        Self::validate_discount(&discount_kind, discount_value, discount_min_qty, &discount_tiers)?;
        Self::validate_variations(&variations)?;
        let mut product = self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("Product not found".into()))?;
        let old_stock = product.stock_quantity;

        product.name = name;
        product.description = description;
        product.category_id = category_id;
        product.subcategory_id = subcategory_id;
        product.price = price;
        product.cost_price = cost_price;
        product.stock_quantity = if unlimited_stock { 0.0 } else { stock_quantity };
        product.min_stock = min_stock;
        product.unlimited_stock = unlimited_stock;
        product.barcode = barcode;
        product.unit = unit;
        product.balance_mode = balance_mode;
        product.image_data = image_data;
        product.cover_color = cover_color;
        product.availability_schedule = availability_schedule;
        product.discount_kind = discount_kind;
        product.discount_value = discount_value;
        product.discount_min_qty = discount_min_qty;
        product.discount_tiers = discount_tiers;
        product.variations = variations;
        product.base.updated_at = chrono::Utc::now().naive_utc();
        product.base.synced = false;

        // Estoque NÃO sincroniza por LWW (§7): mudanças de estoque na edição
        // viram delta no ledger. O `update` mantém o estoque atual e a
        // diferença (se houver) é aplicada via `try_adjust_stock`, que grava
        // o StockMovement atomicamente. Produto ilimitado não tem delta.
        let target_stock = product.stock_quantity;
        let stock_delta = if unlimited_stock { 0.0 } else { target_stock - old_stock };
        if stock_delta.abs() > f64::EPSILON {
            product.stock_quantity = old_stock; // o update não altera o estoque
        }
        self.repo.update(&product).await?;
        if stock_delta.abs() > f64::EPSILON {
            self.repo.try_adjust_stock(company_id, id, stock_delta).await?;
            product.stock_quantity = target_stock; // reflete no retorno
        }
        // Reescreve as associações N:M com a lista atual (lista vazia
        // limpa todas) — mesmo padrão de outras coleções gerenciadas
        // pelo produto.
        self.repo.replace_addon_groups(company_id, product.base.id, &addon_group_ids).await?;
        product.addon_group_ids = addon_group_ids;
        Ok(product)
    }

    /// Validação centralizada dos campos de produto.
    ///
    /// Quando `unlimited_stock=true` o valor de `stock_quantity` deixa de
    /// ser semântico (o produto nunca esgota), então não validamos sinal —
    /// o caller é responsável por normalizar para 0.
    fn validate_product(
        name: &str,
        price: Option<Decimal>,
        cost_price: Option<Decimal>,
        stock_quantity: f64,
        min_stock: f64,
        unlimited_stock: bool,
    ) -> Result<(), CoreError> {
        if name.trim().is_empty() {
            return Err(CoreError::Validation("Product name is required".into()));
        }
        if let Some(p) = price {
            if p < Decimal::ZERO { return Err(CoreError::Validation("Price cannot be negative".into())); }
        }
        if let Some(c) = cost_price {
            if c < Decimal::ZERO { return Err(CoreError::Validation("Cost price cannot be negative".into())); }
        }
        if !unlimited_stock && stock_quantity < 0.0 {
            return Err(CoreError::Validation("Stock quantity cannot be negative".into()));
        }
        if min_stock < 0.0 {
            return Err(CoreError::Validation("Minimum stock cannot be negative".into()));
        }
        Ok(())
    }

    /// Validação dos campos de desconto.
    ///
    /// Regras aplicadas (AI_RULES.md §11):
    /// - `kind` precisa ser um dos 4 valores conhecidos (ou ausente).
    /// - `fixed`/`percent`: `value > 0` (com `percent` em (0, 100)),
    ///   `min_qty` e `tiers` devem ser None.
    /// - `bulk_*`: aceita 2 modos (mutuamente exclusivos):
    ///   (a) tier único legado em `value`/`min_qty`;
    ///   (b) múltiplos tiers em `tiers` (JSON), com `value`/`min_qty` None.
    ///   `tiers` precisa ser array não vazio, ordenado por `min_qty`
    ///   estritamente crescente, cada `value > 0` (em (0,100) para percent).
    fn validate_discount(
        kind: &Option<String>,
        value: Option<Decimal>,
        min_qty: Option<f64>,
        tiers: &Option<String>,
    ) -> Result<(), CoreError> {
        let Some(k) = kind.as_deref() else {
            if value.is_some() || min_qty.is_some() || tiers.is_some() {
                return Err(CoreError::Validation(
                    "Discount fields informed but kind is missing".into(),
                ));
            }
            return Ok(());
        };
        if !matches!(k, "fixed" | "percent" | "bulk_fixed" | "bulk_percent") {
            return Err(CoreError::Validation(format!(
                "Unknown discount_kind: '{k}' (expected fixed|percent|bulk_fixed|bulk_percent)"
            )));
        }
        let is_bulk = k.starts_with("bulk_");
        let is_percent = k == "percent" || k == "bulk_percent";

        if !is_bulk {
            if tiers.is_some() {
                return Err(CoreError::Validation(
                    "tiers is only valid for bulk_* discounts".into(),
                ));
            }
            if min_qty.is_some() {
                return Err(CoreError::Validation(
                    "min_qty is only valid for bulk_* discounts".into(),
                ));
            }
            let v = value.ok_or_else(|| CoreError::Validation(
                "Discount value is required when kind is set".into(),
            ))?;
            Self::check_discount_value(v, is_percent)?;
            return Ok(());
        }

        // bulk_*: modo tier único (legado) OU múltiplos tiers (novo).
        match (tiers, value, min_qty) {
            (Some(json), None, None) => Self::validate_bulk_tiers(json, is_percent),
            (None, Some(v), Some(q)) => {
                if q <= 0.0 {
                    return Err(CoreError::Validation(
                        "bulk_* discount requires min_qty > 0".into(),
                    ));
                }
                Self::check_discount_value(v, is_percent)
            }
            (None, _, _) => Err(CoreError::Validation(
                "bulk_* requires either tiers or (value + min_qty)".into(),
            )),
            (Some(_), _, _) => Err(CoreError::Validation(
                "bulk_* must not mix tiers with legacy value/min_qty".into(),
            )),
        }
    }

    fn check_discount_value(v: Decimal, is_percent: bool) -> Result<(), CoreError> {
        if v < Decimal::ZERO {
            return Err(CoreError::Validation("Discount value cannot be negative".into()));
        }
        if is_percent && (v <= Decimal::ZERO || v >= dec!(100)) {
            return Err(CoreError::Validation(
                "Percent discount must be in (0, 100)".into(),
            ));
        }
        Ok(())
    }

    /// Valida o JSON de `discount_tiers`:
    /// - precisa ser array de objetos `{min_qty, value}`;
    /// - pelo menos 1 tier;
    /// - `min_qty` estritamente crescente (evita tiers duplicados/ambíguos);
    /// - cada `value` passa pelo mesmo crivo de `check_discount_value`.
    fn validate_bulk_tiers(json: &str, is_percent: bool) -> Result<(), CoreError> {
        let parsed: serde_json::Value = serde_json::from_str(json)
            .map_err(|e| CoreError::Validation(format!("discount_tiers invalid JSON: {e}")))?;
        let arr = parsed.as_array().ok_or_else(|| CoreError::Validation(
            "discount_tiers must be an array".into(),
        ))?;
        if arr.is_empty() {
            return Err(CoreError::Validation("discount_tiers cannot be empty".into()));
        }
        let mut prev_qty = f64::NEG_INFINITY;
        for (idx, entry) in arr.iter().enumerate() {
            let obj = entry.as_object().ok_or_else(|| CoreError::Validation(format!(
                "discount_tiers[{idx}] must be an object"
            )))?;
            let q = obj.get("min_qty").and_then(|v| v.as_f64()).ok_or_else(|| {
                CoreError::Validation(format!("discount_tiers[{idx}].min_qty missing"))
            })?;
            let v = obj.get("value").and_then(|x| x.as_f64()).and_then(rust_decimal::prelude::FromPrimitive::from_f64).ok_or_else(|| {
                CoreError::Validation(format!("discount_tiers[{idx}].value missing"))
            })?;
            if q <= 0.0 {
                return Err(CoreError::Validation(format!(
                    "discount_tiers[{idx}].min_qty must be > 0"
                )));
            }
            if q <= prev_qty {
                return Err(CoreError::Validation(
                    "discount_tiers must be strictly increasing by min_qty".into(),
                ));
            }
            Self::check_discount_value(v, is_percent)?;
            prev_qty = q;
        }
        Ok(())
    }

    /// Valida o JSON de `variations` (Fase 5).
    ///
    /// Regras aplicadas (AI_RULES.md §11):
    /// - `None` = produto sem variações (válido).
    /// - JSON precisa ser array de objetos com `title`, `selection`,
    ///   `required` e `options`.
    /// - `selection` ∈ {"single", "multi", "max_value"}.
    /// - `options` não pode ser vazio (senão a variação não tem sentido).
    /// - Cada opção: `name` não-vazio, `price >= 0`.
    /// - `title` não-vazio (depois de trim).
    /// - Sem títulos duplicados entre variações do mesmo produto
    ///   (ambíguo para o cliente).
    fn validate_variations(json: &Option<String>) -> Result<(), CoreError> {
        let Some(raw) = json.as_deref() else { return Ok(()); };
        let trimmed = raw.trim();
        if trimmed.is_empty() { return Ok(()); }
        let parsed: serde_json::Value = serde_json::from_str(trimmed)
            .map_err(|e| CoreError::Validation(format!("variations invalid JSON: {e}")))?;
        let arr = parsed.as_array().ok_or_else(|| CoreError::Validation(
            "variations must be an array".into(),
        ))?;
        let mut seen_titles: Vec<String> = Vec::with_capacity(arr.len());
        for (idx, entry) in arr.iter().enumerate() {
            let obj = entry.as_object().ok_or_else(|| CoreError::Validation(format!(
                "variations[{idx}] must be an object"
            )))?;
            let title = obj.get("title").and_then(|v| v.as_str())
                .map(str::trim).unwrap_or("");
            if title.is_empty() {
                return Err(CoreError::Validation(format!(
                    "variations[{idx}].title is required"
                )));
            }
            let lower = title.to_lowercase();
            if seen_titles.contains(&lower) {
                return Err(CoreError::Validation(format!(
                    "Duplicate variation title: '{title}'"
                )));
            }
            seen_titles.push(lower);
            let selection = obj.get("selection").and_then(|v| v.as_str()).unwrap_or("");
            if !matches!(selection, "single" | "multi" | "max_value") {
                return Err(CoreError::Validation(format!(
                    "variations[{idx}].selection must be single|multi|max_value"
                )));
            }
            // `required` é boolean; default false se ausente.
            let _required = obj.get("required").and_then(|v| v.as_bool()).unwrap_or(false);
            let options = obj.get("options").and_then(|v| v.as_array())
                .ok_or_else(|| CoreError::Validation(format!(
                    "variations[{idx}].options is required (non-empty array)"
                )))?;
            if options.is_empty() {
                return Err(CoreError::Validation(format!(
                    "variations[{idx}].options cannot be empty"
                )));
            }
            // Mín./máx. de seleções (Fase 5B) — só para multi/max_value.
            // Ausentes/0 = sem restrição (compatível com dados antigos).
            // `max_select == 0` = sem limite superior.
            if matches!(selection, "multi" | "max_value") {
                let n = options.len() as i64;
                let min_sel = obj.get("min_select").and_then(|v| v.as_i64()).unwrap_or(0);
                let max_sel = obj.get("max_select").and_then(|v| v.as_i64()).unwrap_or(0);
                if min_sel < 0 {
                    return Err(CoreError::Validation(format!(
                        "variations[{idx}].min_select cannot be negative"
                    )));
                }
                if min_sel > n {
                    return Err(CoreError::Validation(format!(
                        "variations[{idx}].min_select exceeds number of options"
                    )));
                }
                if max_sel < 0 {
                    return Err(CoreError::Validation(format!(
                        "variations[{idx}].max_select cannot be negative"
                    )));
                }
                if max_sel > 0 {
                    if max_sel > n {
                        return Err(CoreError::Validation(format!(
                            "variations[{idx}].max_select exceeds number of options"
                        )));
                    }
                    if max_sel < min_sel {
                        return Err(CoreError::Validation(format!(
                            "variations[{idx}].max_select must be >= min_select"
                        )));
                    }
                }
            }
            for (opt_idx, opt) in options.iter().enumerate() {
                let opt_obj = opt.as_object().ok_or_else(|| CoreError::Validation(format!(
                    "variations[{idx}].options[{opt_idx}] must be an object"
                )))?;
                let opt_name = opt_obj.get("name").and_then(|v| v.as_str())
                    .map(str::trim).unwrap_or("");
                if opt_name.is_empty() {
                    return Err(CoreError::Validation(format!(
                        "variations[{idx}].options[{opt_idx}].name is required"
                    )));
                }
                let opt_price = opt_obj.get("price").and_then(|v| v.as_f64())
                    .ok_or_else(|| CoreError::Validation(format!(
                        "variations[{idx}].options[{opt_idx}].price is required"
                    )))?;
                if opt_price < 0.0 {
                    return Err(CoreError::Validation(format!(
                        "variations[{idx}].options[{opt_idx}].price cannot be negative"
                    )));
                }
            }
        }
        Ok(())
    }

    /// Remoção lógica (soft delete).
    pub async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("Product not found".into()))?;
        self.repo.soft_delete(company_id, id).await
    }

    /// Busca produtos ainda não sincronizados (§7).
    pub async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Product>, CoreError> {
        self.repo.find_unsynced(company_id).await
    }

    /// Marca produto como sincronizado (§7).
    pub async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        self.repo.mark_synced(company_id, id, updated_at).await
    }

    /// Busca produtos atualizados após o timestamp (§7 — sync pull).
    pub async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
    ) -> Result<Vec<Product>, CoreError> {
        self.repo.find_updated_since(company_id, since).await
    }

    // ── Movimentos de estoque (ledger — §7) ──
    /// Movimentos pendentes de push.
    pub async fn find_unsynced_stock_movements(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<StockMovement>, CoreError> {
        self.repo.find_unsynced_stock_movements(company_id).await
    }

    /// Marca movimento como sincronizado (condicional ao updated_at — §7.6).
    pub async fn mark_stock_movement_synced(
        &self,
        company_id: Uuid,
        id: Uuid,
        updated_at: chrono::NaiveDateTime,
    ) -> Result<(), CoreError> {
        self.repo.mark_stock_movement_synced(company_id, id, updated_at).await
    }

    /// Aplica um movimento recebido de forma idempotente (valida o tenant).
    pub async fn apply_stock_movement(
        &self,
        company_id: Uuid,
        movement: StockMovement,
    ) -> Result<(), CoreError> {
        if movement.base.company_id != company_id {
            return Err(CoreError::Validation("Company mismatch".into()));
        }
        self.repo.apply_stock_movement(&movement).await
    }

    /// Movimentos alterados após `since` (pull servidor→desktop).
    pub async fn find_stock_movements_updated_since(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
    ) -> Result<Vec<StockMovement>, CoreError> {
        self.repo.find_stock_movements_updated_since(company_id, since).await
    }

    /// Retorna apenas produtos ativos para exibição no catálogo público.
    ///
    /// Regras aplicadas (AI_RULES.md §3, §8):
    /// - Regra de negócio (só ativos) encapsulada no service
    /// - Catálogo web nunca vê produtos inativos
    pub async fn find_active(&self, company_id: Uuid) -> Result<Vec<Product>, CoreError> {
        self.repo.find_active(company_id).await
    }

    /// Alterna estado ativo/inativo do produto (global: cardápio web + PDV).
    ///
    /// Regras aplicadas (AI_RULES.md §8, §11):
    /// - Valida existência antes de alterar
    /// - Persistência delegada ao repository
    pub async fn toggle_active(&self, company_id: Uuid, id: Uuid, active: bool) -> Result<(), CoreError> {
        self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("Product not found".into()))?;
        self.repo.toggle_active(company_id, id, active).await
    }

    /// Alterna visibilidade do produto no cardápio web.
    ///
    /// Regras aplicadas (AI_RULES.md §8, §11):
    /// - Independente de `active`: produto pode estar ativo (PDV vê) mas
    ///   oculto na web. Útil para itens só vendidos no balcão.
    /// - Valida existência antes de alterar.
    pub async fn toggle_web_visible(&self, company_id: Uuid, id: Uuid, visible: bool) -> Result<(), CoreError> {
        self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("Product not found".into()))?;
        self.repo.toggle_web_visible(company_id, id, visible).await
    }

    /// Ajusta o estoque do produto em `delta` (positivo restitui, negativo decrementa).
    ///
    /// Regras aplicadas (AI_RULES.md §1, §4, §11):
    /// - Operação delegada ao repo via `try_adjust_stock` para garantir
    ///   atomicidade: o UPDATE SQL combina o `delta` e a validação de
    ///   "não-negativo" numa única transação, eliminando a janela de
    ///   race entre dois clientes que vendem o mesmo produto em paralelo.
    /// - Estoque ilimitado é no-op (sem leitura/escrita).
    /// - `synced = false` é setado pelo próprio UPDATE.
    pub async fn adjust_stock(
        &self,
        company_id: Uuid,
        id: Uuid,
        delta: f64,
    ) -> Result<(), CoreError> {
        use super::repository::StockAdjustResult::*;
        match self.repo.try_adjust_stock(company_id, id, delta).await? {
            Adjusted | Unlimited => Ok(()),
            NotFound => Err(CoreError::NotFound("Product not found".into())),
            Insufficient => {
                // Hidrata o nome do produto apenas para uma mensagem
                // de erro mais clara — só ocorre quando o estoque é
                // de fato insuficiente.
                let name = self.repo.find_by_id(company_id, id).await?
                    .map(|p| p.name)
                    .unwrap_or_else(|| id.to_string());
                Err(CoreError::Validation(format!(
                    "Estoque insuficiente para '{}' (necessário: {})",
                    name, -delta
                )))
            }
        }
    }

    /// Upsert de sincronização (§7.7 — last-write-wins).
    ///
    /// Regras aplicadas (AI_RULES.md §7.7, §11):
    /// - Valida company_id contra o tenant autenticado.
    /// - Marca synced = true antes de persistir.
    /// - Repository resolve conflito via updated_at.
    /// - A junção N:M `product_addon_groups` SÓ é reescrita quando o
    ///   produto recebido é efetivamente mais recente que o local.
    ///   Sem essa guarda, dois clientes editando o mesmo produto em
    ///   janelas distintas se sobrescreveriam mutuamente (o que chegou
    ///   por último venceria mesmo sendo a versão mais antiga).
    pub async fn sync_upsert(
        &self,
        company_id: Uuid,
        mut product: Product,
    ) -> Result<(), CoreError> {
        if product.base.company_id != company_id {
            return Err(CoreError::Validation("Company mismatch".into()));
        }
        product.base.synced = true;
        let group_ids = product.addon_group_ids.clone();
        let product_id = product.base.id;
        let incoming_updated_at = product.base.updated_at;
        // Lê o existente ANTES do upsert para comparar.
        let existing_updated_at = self.repo.find_by_id(company_id, product_id).await?
            .map(|p| p.base.updated_at);
        self.repo.sync_upsert(&product).await?;
        // Reescreve junção apenas se a nossa versão venceu o
        // last-write-wins do repo. Caso contrário, mantemos o estado
        // local (que reflete a versão vencedora).
        let won = existing_updated_at
            .map(|local| incoming_updated_at > local)
            .unwrap_or(true); // produto novo: sempre escreve
        if won {
            self.repo.replace_addon_groups(company_id, product_id, &group_ids).await?;
        }
        Ok(())
    }
}
