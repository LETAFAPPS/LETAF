use std::collections::HashMap;

use slint::SharedPixelBuffer;
use uuid::Uuid;

use letaf_core::product::model::Product;



#[derive(Clone)]
pub(crate) struct CartItem {
    pub(crate) line_id: Uuid,
    pub(crate) product_id: Uuid,
    pub(crate) name: String,
    pub(crate) qty: f64,
    pub(crate) unit_price: f64,
    /// Soma dos adicionais/variações da linha (R$ por unidade). O
    /// desconto do produto incide só no preço base — guardar os extras
    /// separados permite recomputar `unit_price` quando a quantidade
    /// muda (descontos `bulk_*` dependem da qty) e exibir o preço de
    /// tabela (base cheio + extras) riscado na UI.
    pub(crate) extras: f64,
    pub(crate) addons_summary: String,
    pub(crate) addons_json: Option<String>,
}

impl CartItem {
    /// Recalcula `unit_price` para a quantidade atual da linha:
    /// preço efetivo do produto (desconto conforme qty) + extras.
    /// Chamado sempre que a qty muda (add/merge, +, −, modal).
    pub(crate) fn reprice(&mut self, products: &[Product]) {
        if let Some(p) = products.iter().find(|p| p.base.id == self.product_id) {
            use rust_decimal::prelude::ToPrimitive;
            let base = letaf_core::discount::effective_unit_price(p, self.qty)
                .to_f64()
                .unwrap_or(0.0);
            self.unit_price = base + self.extras;
        }
    }

    /// Preço unitário de tabela (produto sem desconto + extras), para a
    /// UI exibir riscado quando difere do `unit_price` cobrado.
    pub(crate) fn list_unit_price(&self, products: &[Product]) -> f64 {
        use rust_decimal::prelude::ToPrimitive;
        products
            .iter()
            .find(|p| p.base.id == self.product_id)
            .and_then(|p| p.price)
            .and_then(|v| v.to_f64())
            .map(|base| base + self.extras)
            .unwrap_or(self.unit_price)
    }
}

pub(crate) struct PdvState {
    pub(crate) products_all: Vec<Product>,
    pub(crate) categories: Vec<(Uuid, String)>,
    pub(crate) active_category_ids: Vec<Uuid>,
    pub(crate) search_query: String,
    pub(crate) cart: Vec<CartItem>,
    /// Snapshot dos clientes da empresa (para o picker buscar
    /// localmente sem ir ao banco a cada tecla).
    pub(crate) customers_all: Vec<(Uuid, String, Option<String>, Option<String>)>,
    /// Endereços do cliente atualmente selecionado. Recarregado em
    /// cada `pick_customer`; resetado em `clear_customer`. Mantido
    /// como `CustomerAddress` (não tupla) para a função `use_address`
    /// ter acesso a todos os campos via id.
    pub(crate) current_customer_addresses: Vec<letaf_core::customer_address::model::CustomerAddress>,
    /// Cache de imagens decodificadas (`product_id` → `SharedPixelBuffer`).
    /// Preenchido no refresh (decodifica `image_data` base64 uma única
    /// vez). `SharedPixelBuffer` é clonável barato (Rc interno), então
    /// passar para `Image::from_rgba8` no `apply_state_to_ui` não
    /// realoca pixels.
    pub(crate) image_cache: HashMap<Uuid, SharedPixelBuffer<slint::Rgba8Pixel>>,
    /// Valor do desconto digitado pelo operador (R$).
    pub(crate) discount_value: f64,
    /// Valor adicional/acréscimo digitado pelo operador (R$) — soma ao total.
    pub(crate) additional_value: f64,
    /// Valor pago em dinheiro digitado pelo operador (R$).
    pub(crate) amount_paid: f64,
    /// Taxa de entrega (frete) configurada na empresa (R$). Carregada no
    /// refresh a partir de `company.delivery_fee`. Aplicada ao total
    /// somente quando o pedido é do tipo entrega (§11: fonte de verdade
    /// é a config local, não um valor digitado na venda).
    pub(crate) delivery_fee: f64,
    /// Largura atual (em px lógicos) do painel de categorias, recebida
    /// do Slint via `cats-width-changed`. Usada para decidir se os
    /// chips cabem numa linha ou precisam ser divididos em duas. 0 =
    /// ainda não medido (primeira renderização) → assume largo.
    pub(crate) cats_width: f32,
}

impl PdvState {
    pub(crate) fn new() -> Self {
        Self {
            products_all: Vec::new(),
            categories: Vec::new(),
            active_category_ids: Vec::new(),
            search_query: String::new(),
            cart: Vec::new(),
            customers_all: Vec::new(),
            current_customer_addresses: Vec::new(),
            image_cache: HashMap::new(),
            discount_value: 0.0,
            additional_value: 0.0,
            amount_paid: 0.0,
            delivery_fee: 0.0,
            cats_width: 0.0,
        }
    }

    pub(crate) fn subtotal(&self) -> f64 {
        self.cart.iter().map(|l| l.qty * l.unit_price).sum()
    }

    /// Taxa de entrega efetiva: só cobra quando o pedido é de entrega.
    pub(crate) fn delivery_fee_for(&self, is_delivery: bool) -> f64 {
        if is_delivery { self.delivery_fee.max(0.0) } else { 0.0 }
    }

    /// Total base (sem taxa de entrega) — itens - desconto + acréscimo.
    pub(crate) fn total(&self) -> f64 {
        (self.subtotal() - self.discount_value + self.additional_value).max(0.0)
    }

    /// Total considerando a taxa de entrega quando aplicável.
    pub(crate) fn total_with_delivery(&self, is_delivery: bool) -> f64 {
        (self.total() + self.delivery_fee_for(is_delivery)).max(0.0)
    }
}

/// Parseia o valor monetário digitado pelo operador (aceita `,` e `.`
/// como separadores decimais). Devolve 0.0 quando o input está vazio
/// ou inválido — sem propagar erro pra UI; cálculo cai num valor
/// neutro até o operador corrigir.
pub(crate) fn parse_amount(raw: &str) -> f64 {
    raw.trim()
        .replace(',', ".")
        .parse::<f64>()
        .unwrap_or(0.0)
        .max(0.0)
}

/// Formata um valor em reais sem sinal negativo "fantasma".
/// `format!("R$ {:.2}", -0.0_f64)` produz `"R$ -0.00"`; aqui
/// "snap to zero" para qualquer valor com |v| < 0.005 evita o
/// negativo visual e centavos próximos de zero.
pub(crate) fn fmt_brl(v: f64) -> String {
    let safe = if v.abs() < 0.005 || !v.is_finite() { 0.0 } else { v };
    format!("R$ {:.2}", safe)
}

