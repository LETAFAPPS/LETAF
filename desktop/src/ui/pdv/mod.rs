//! Callbacks da tela PDV (Ponto de Venda).
//!
//! Layout: produtos (esquerda) + carrinho integrado (direita com tipo,
//! cliente, endereço, pagamento). UI em `pages/pdv_page.slint` + modais.
//!
//! AI_RULES.md §1/§14: zero lógica de negócio na UI — validações, baixa
//! de estoque e criação do `Order` ficam no service (`create_pdv`).
//!
//! Dividido por responsabilidade (§8, §9):
//! - `state`: `PdvState`/`CartItem` em memória + helpers de valor
//! - `catalog`: orquestrador (`setup_pdv`), busca, categorias e carrinho-add
//! - `cart`: operações de linha do carrinho (inc/dec/remover/limpar)
//! - `finalize`: confirmação da venda → `order_service::create_pdv`
//! - `customer`: picker de cliente, carteira e endereço
//! - `view`: renderização do estado na UI (`apply_state_to_ui`)

mod cart;
mod catalog;
mod customer;
mod finalize;
mod state;
mod view;

pub(crate) use catalog::setup_pdv;
