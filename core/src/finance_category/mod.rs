//! Categorias para classificação de lançamentos financeiros
//! (contas a pagar / a receber).
//!
//! Regras aplicadas (AI_RULES.md §1, §6, §9):
//! - Domínio puro: model + repository (trait) + service.
//! - Entidade com `BaseFields` (UUID, company_id, soft delete, sync).
//! - Sem dependência de UI/banco aqui (apenas o trait do repositório).

pub mod model;
pub mod repository;
pub mod service;
