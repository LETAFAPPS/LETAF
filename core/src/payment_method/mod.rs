//! Catálogo de formas de pagamento da empresa.
//!
//! Regras aplicadas (AI_RULES.md §1, §6, §9):
//! - Domínio puro: model + repository (trait) + service.
//! - Sem dados sensíveis (CVV, número completo do cartão). Apenas
//!   catalogação visual até a tokenização do gateway entrar.

pub mod model;
pub mod repository;
pub mod service;
