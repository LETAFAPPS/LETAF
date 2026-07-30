//! Carteira do estabelecimento (tesouraria) — saldo inicial da empresa.
//!
//! Regras aplicadas (AI_RULES.md §1, §6, §9):
//! - Domínio puro: model + repository (trait) + service.
//! - Entidade com `BaseFields` (UUID, company_id, soft delete, sync).
//! - SINGLETON por empresa: no máximo UMA carteira por `company_id`
//!   (unique na migration; service valida na criação).
//! - `initial_balance >= 0` — validado no service (§11: nunca confiar
//!   no frontend).

pub mod model;
pub mod repository;
pub mod service;
