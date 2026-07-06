//! Lançamentos financeiros (contas a pagar / receber).
//!
//! Regras aplicadas (AI_RULES.md §1, §6, §9):
//! - Domínio puro: model + repository (trait) + service.
//! - Entidade com `BaseFields` (UUID, company_id, soft delete, sync).
//! - Suporte a parcelamento e recorrência via `parent_id` (relação
//!   pai → filhos com cópia dos campos imutáveis), permitindo cancelar
//!   apenas uma parcela sem perder o cabeçalho.

pub mod model;
pub mod repository;
pub mod service;
