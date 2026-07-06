use std::fmt;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal_macros::dec;
use rust_decimal::Decimal;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::BaseFields;

/// Modo de codificação do EAN-13 de balança (padrão brasileiro prefixo `2`).
///
/// Regras aplicadas (AI_RULES.md §11):
/// - Só faz sentido quando `Product.unit == "kg"`; ignorado nas outras unidades.
/// - `Weight`: os 5 dígitos variáveis representam peso em gramas — o PDV
///   calcula `total = price_per_kg * (raw_value / 1000.0)`.
/// - `Price`: os 5 dígitos representam preço total em centavos — o PDV usa
///   `total = raw_value / 100.0` (preço já fechado pela balança).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BalanceMode {
    #[default]
    Weight,
    Price,
}

impl BalanceMode {
    pub fn as_db_str(&self) -> &'static str {
        match self {
            Self::Weight => "weight",
            Self::Price  => "price",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "weight" => Some(Self::Weight),
            "price"  => Some(Self::Price),
            _ => None,
        }
    }

    /// Rótulo em pt-BR para exibição na UI.
    pub fn label_pt_br(&self) -> &'static str {
        match self {
            Self::Weight => "Peso",
            Self::Price  => "Valor",
        }
    }
}

impl fmt::Display for BalanceMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_db_str())
    }
}

/// Entidade Produto — item comercializado pela empresa.
///
/// Regras aplicadas (AI_RULES.md §6, §11):
/// - Campos base obrigatórios (UUID, company_id, timestamps, synced)
/// - Campos de domínio: name, description, price, cost_price, stock_quantity,
///   barcode, unit, active, web_visible, balance_mode, image_data
/// - `active`: ativo global (cardápio + PDV). false esconde em todo lugar.
/// - `web_visible`: visibilidade no cardápio web. false + active=true → só PDV.
/// - `barcode`: código de barras (EAN/UPC). Usado pelo leitor no PDV (Fase 2).
/// - `balance_mode`: para `unit == "kg"`, indica se a balança encoda peso ou
///   preço no EAN-13. Ignorado para outras unidades.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Product {
    #[serde(flatten)]
    pub base: BaseFields,
    pub name: String,
    pub description: Option<String>,
    pub category_id: Option<Uuid>,
    pub subcategory_id: Option<Uuid>,
    pub price: Option<Decimal>,
    pub cost_price: Option<Decimal>,
    pub stock_quantity: f64,
    /// Estoque mínimo desejado. Quando `0 < stock_quantity <= min_stock`
    /// o produto entra em "estoque baixo"; abaixo disso a UI sugere a
    /// compra de `min_stock - stock_quantity` unidades. `0` (default)
    /// desliga o alerta — o produto só é "esgotado" em `stock <= 0`.
    /// Irrelevante quando `unlimited_stock = true`.
    #[serde(default)]
    pub min_stock: f64,
    /// Quando `true`, o produto é tratado como infinito: filtros de
    /// catálogo o mantêm sempre disponível, o carrinho web não impõe
    /// teto e o `OrderService::create` pula a baixa de estoque.
    /// Útil para itens preparados sob demanda (pizzas, sucos) que não
    /// fazem sentido contabilizar como estoque físico.
    #[serde(default)]
    pub unlimited_stock: bool,
    pub barcode: Option<String>,
    pub unit: String,
    #[serde(default = "default_true")]
    pub active: bool,
    #[serde(default = "default_true")]
    pub web_visible: bool,
    #[serde(default)]
    pub balance_mode: BalanceMode,
    #[serde(default)]
    pub image_data: Option<String>,
    /// Cor de fundo detectada nas bordas da imagem do produto, em hex
    /// `#RRGGBB`. Populada pela heurística no upload:
    /// - `None`: imagem transparente (PNG sem fundo) ou indetectável →
    ///   o card mostra a cor neutra do tema (igual ao placeholder).
    /// - `Some("#RRGGBB")`: imagem opaca com bordas uniformes naquela cor
    ///   → o card pinta com essa cor, eliminando a "costura" visual.
    #[serde(default)]
    pub cover_color: Option<String>,
    /// Janela de disponibilidade do produto no cardápio web, por dia da
    /// semana. `None` = sempre disponível (default — sem agenda).
    ///
    /// `Some(json)` = string JSON com array de 7 entradas (uma por dia,
    /// 0 = domingo até 6 = sábado), cada uma com:
    /// - `open`: "HH:MM"
    /// - `close`: "HH:MM"
    /// - `active`: bool (false desativa o dia inteiro)
    ///
    /// Mantemos o schedule como string para evitar uma tabela N:1 — a
    /// consulta sempre carrega o produto inteiro, e o cliente web parseia
    /// localmente para decidir se mostra "Adicionar" ou "Indisponível".
    #[serde(default)]
    pub availability_schedule: Option<String>,
    /// Tipo de desconto aplicado ao produto no cardápio web.
    /// Valores válidos: `"fixed"`, `"percent"`, `"bulk_fixed"`, `"bulk_percent"`.
    /// `None` = sem desconto.
    #[serde(default)]
    pub discount_kind: Option<String>,
    /// Valor do desconto. Em R$ para `fixed`/`bulk_fixed`; em % (0..100)
    /// para `percent`/`bulk_percent`. `None` quando `discount_kind` é `None`.
    #[serde(default)]
    pub discount_value: Option<Decimal>,
    /// Quantidade mínima para aplicar descontos `bulk_*` quando há um único
    /// tier legado. `None` quando irrelevante ou quando `discount_tiers` é
    /// usado (modo multi-tier).
    #[serde(default)]
    pub discount_min_qty: Option<f64>,
    /// Tiers para `bulk_fixed`/`bulk_percent`: JSON com array
    /// `[{"min_qty": f64, "value": f64}, ...]`, ordenado por `min_qty`
    /// crescente. `None` para `fixed`/`percent` e quando ainda não há
    /// tiers configurados.
    ///
    /// Optei por uma única coluna `TEXT` (mesmo padrão de
    /// `availability_schedule`) ao invés de uma tabela N:1 — a query
    /// sempre carrega o produto inteiro e o cliente parseia localmente
    /// para decidir o tier vencente em runtime.
    #[serde(default)]
    pub discount_tiers: Option<String>,
    /// IDs dos `AddonGroup` associados ao produto. Persistidos na
    /// tabela de junção `product_addon_groups` (N:M) — não em coluna
    /// do `products`. O repository popula este vetor no
    /// `find_by_id`/`find_all`/etc. e o reescreve via
    /// `replace_addon_groups` no `Product.update`.
    ///
    /// Optei pela tabela de junção (não JSON) para preservar integridade
    /// referencial: grupo deletado é refletido pelo soft-delete + filtro
    /// no JOIN.
    #[serde(default)]
    pub addon_group_ids: Vec<Uuid>,
    /// Variações do produto (Fase 5): "Tamanho", "Sabor" etc. JSON
    /// `[{title, selection, required, options:[{name, price}]}]`.
    /// Diferente dos adicionais, é **per-produto** (não compartilhada),
    /// por isso fica como JSON no próprio produto — mesmo padrão de
    /// `discount_tiers` e `availability_schedule`.
    ///
    /// `selection` ∈ {"single", "multi", "max_value"}:
    /// - "single": radio, cliente escolhe 1 opção.
    /// - "multi": checkbox, cliente escolhe N; todos os `price` somam.
    /// - "max_value": checkbox, cliente escolhe N mas só o `price`
    ///   da opção de maior valor entra no `unit_price` final.
    #[serde(default)]
    pub variations: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Product {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
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
    ) -> Self {
        Self {
            base: BaseFields::new(company_id),
            name,
            description,
            category_id,
            subcategory_id,
            price,
            cost_price,
            stock_quantity,
            min_stock,
            unlimited_stock,
            barcode,
            unit,
            active: true,
            web_visible: true,
            balance_mode,
            image_data,
            cover_color,
            availability_schedule,
            discount_kind,
            discount_value,
            discount_min_qty,
            discount_tiers,
            // Associações N:M são definidas via repo `replace_addon_groups`
            // após o create — não cabem no construtor in-memory.
            addon_group_ids: Vec::new(),
            variations: None,
        }
    }
}

/// Status de estoque derivado — fonte única para UI e relatórios
/// (AI_RULES.md §1, §14: regra de negócio fica no core, a camada de
/// apresentação só formata).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StockStatus {
    /// `unlimited_stock = true`: nunca esgota.
    Unlimited,
    /// `stock_quantity <= 0`: indisponível (venda bloqueada por padrão).
    Out,
    /// `0 < stock_quantity <= min_stock` (com `min_stock > 0`): repor.
    Low,
    /// Acima do mínimo.
    Ok,
}

impl StockStatus {
    /// Slug estável consumido pela UI (não traduzir aqui — §14).
    pub fn as_slug(&self) -> &'static str {
        match self {
            Self::Unlimited => "unlimited",
            Self::Out => "out",
            Self::Low => "low",
            Self::Ok => "ok",
        }
    }
}

impl Product {
    /// Margem de lucro em % sobre o preço de venda.
    ///
    /// `None` quando faltam dados para o cálculo (sem preço, sem custo
    /// ou preço zero) — a UI mostra "—" nesse caso. Centralizado aqui
    /// para que web/desktop nunca recalculem (AI_RULES.md §1).
    pub fn margin_pct(&self) -> Option<f64> {
        let price = self.price?;
        let cost = self.cost_price?;
        if price <= Decimal::ZERO {
            return None;
        }
        ((price - cost) / price * dec!(100)).to_f64()
    }

    /// Lucro unitário absoluto (preço − custo). `None` se faltar dado.
    pub fn margin_amount(&self) -> Option<f64> {
        (self.price? - self.cost_price?).to_f64()
    }

    /// Classifica o estoque atual segundo `min_stock`/`unlimited_stock`.
    pub fn stock_status(&self) -> StockStatus {
        if self.unlimited_stock {
            return StockStatus::Unlimited;
        }
        if self.stock_quantity <= 0.0 {
            return StockStatus::Out;
        }
        if self.min_stock > 0.0 && self.stock_quantity <= self.min_stock {
            return StockStatus::Low;
        }
        StockStatus::Ok
    }

    /// Quantas unidades comprar para voltar ao mínimo. `0.0` quando o
    /// estoque já está no nível ou o produto é ilimitado / sem mínimo.
    pub fn purchase_suggestion(&self) -> f64 {
        if self.unlimited_stock || self.min_stock <= 0.0 {
            return 0.0;
        }
        (self.min_stock - self.stock_quantity).max(0.0)
    }
}
