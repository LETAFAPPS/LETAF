//! Gateway de pagamento abstrato — primeira implementação é Efi (PIX).
//!
//! Regras aplicadas (AI_RULES.md §1, §9, §10):
//! - Domínio puro: model + trait + service.
//! - Implementações concretas (Efi, mock para testes) ficam em
//!   `server/src/integrations/<gateway>/`. Core não conhece HTTP.
//! - Catálogo expansível: trocar de gateway no futuro só exige uma
//!   nova impl da trait.

pub mod card;
pub mod gateway;
pub mod model;
pub mod pix_auto;
pub mod repository;
pub mod service;
