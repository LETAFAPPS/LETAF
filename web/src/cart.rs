//! Carrinho client-side: estado compartilhado por contexto + persistência
//! em localStorage. AI_RULES §11 (frontend burro): o carrinho é só UI; o
//! preço/total exibido é ergonomia — o backend revalida tudo no checkout
//! (`verify_item_prices`/`validate_variations`). Sem lógica de negócio
//! autoritativa aqui. No SSR o carrinho nasce vazio; um `Effect` carrega
//! o localStorage no cliente após a hidratação (sem mismatch).

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use crate::api::CatalogProduct;
use crate::discount;

/// Adicional escolhido — snapshot (nome+preço) no momento da inclusão.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SelectedAddon {
    pub name: String,
    pub price: f64,
}

/// Item do carrinho. `addons` é a foto da escolha; linhas com produto +
/// adicionais iguais se fundem.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CartItem {
    pub product: CatalogProduct,
    pub quantity: f64,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub addons: Vec<SelectedAddon>,
}

impl CartItem {
    /// Preço unitário: desconto aplica só sobre o preço base; adicionais
    /// somam por cima (padrão delivery).
    pub fn unit_price(&self) -> f64 {
        let base = discount::effective_unit_price(&self.product, self.quantity);
        let addons: f64 = self.addons.iter().map(|a| a.price).sum();
        base + addons
    }

    pub fn subtotal(&self) -> f64 {
        self.unit_price() * self.quantity
    }

    /// Funde linhas só quando é o mesmo produto E o mesmo conjunto de
    /// adicionais (mesma ordem).
    pub fn matches(&self, product_id: &str, addons: &[SelectedAddon]) -> bool {
        self.product.id == product_id && self.addons == *addons
    }

    /// Serializa os adicionais em JSON `[{name, price}]` para o backend
    /// (`order_items.addons_json`). `None` quando não há adicionais.
    pub fn addons_json(&self) -> Option<String> {
        if self.addons.is_empty() {
            return None;
        }
        let arr: Vec<serde_json::Value> = self
            .addons
            .iter()
            // Preço como string decimal (2 casas) — sem f64 no JSON gravado.
            .map(|a| serde_json::json!({ "name": a.name, "price": format!("{:.2}", a.price) }))
            .collect();
        serde_json::to_string(&serde_json::Value::Array(arr)).ok()
    }
}

/// Contexto do carrinho (compartilhado pela árvore Leptos).
#[derive(Clone, Copy)]
pub struct Cart(pub RwSignal<Vec<CartItem>>);

impl Cart {
    /// Adiciona um produto; funde com a linha de mesmo produto+adicionais.
    pub fn add(&self, product: CatalogProduct, quantity: f64, addons: Vec<SelectedAddon>) {
        self.0.update(|items| {
            if let Some(it) = items.iter_mut().find(|it| it.matches(&product.id, &addons)) {
                it.quantity += quantity;
            } else {
                items.push(CartItem {
                    product,
                    quantity,
                    notes: None,
                    addons,
                });
            }
        });
        self.persist();
    }

    /// Soma `delta` à quantidade da linha `idx` (lê a atual, evitando
    /// stale entre renders); quando chega a `≤ 0`, remove a linha.
    pub fn bump(&self, idx: usize, delta: f64) {
        self.0.update(|items| {
            let Some(q) = items.get(idx).map(|it| it.quantity + delta) else {
                return;
            };
            if q <= 0.0 {
                items.remove(idx);
            } else {
                items[idx].quantity = q;
            }
        });
        self.persist();
    }

    /// Esvazia o carrinho (após enviar o pedido).
    pub fn clear(&self) {
        self.0.set(Vec::new());
        self.persist();
    }

    /// Total de unidades (badge).
    pub fn count(&self) -> f64 {
        self.0.with(|items| items.iter().map(|i| i.quantity).sum())
    }

    /// Total monetário (soma dos subtotais).
    pub fn total(&self) -> f64 {
        self.0.with(|items| items.iter().map(|i| i.subtotal()).sum())
    }

    fn persist(&self) {
        save(&self.0.get_untracked());
    }
}

#[cfg(feature = "hydrate")]
const KEY: &str = "letaf:cart";

#[cfg(feature = "hydrate")]
pub fn load() -> Vec<CartItem> {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item(KEY).ok().flatten())
        .and_then(|json| serde_json::from_str::<Vec<CartItem>>(&json).ok())
        .unwrap_or_default()
}

#[cfg(not(feature = "hydrate"))]
pub fn load() -> Vec<CartItem> {
    Vec::new()
}

#[cfg(feature = "hydrate")]
fn save(items: &[CartItem]) {
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        if let Ok(json) = serde_json::to_string(items) {
            let _ = storage.set_item(KEY, &json);
        }
    }
}

#[cfg(not(feature = "hydrate"))]
fn save(_items: &[CartItem]) {}
