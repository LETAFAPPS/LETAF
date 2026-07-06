//! Carteira do cliente — saldo + livro-razão de movimentações.
//!
//! Regras aplicadas (AI_RULES.md §1, §6, §9):
//! - Domínio puro: model + repository (trait) + service.
//! - Entidades com `BaseFields` (UUID, company_id, soft delete, sync).
//! - Saldo pode ser negativo: representa fiado (limite configurável
//!   via `credit_limit`). Sem limite (`credit_limit = 0`) significa
//!   "não é permitido sair do positivo" — service valida no saque.
//! - `WalletMovement` é imutável após criação exceto pelo `synced`
//!   (auditoria). Toda operação balance ↔ movement roda em transação
//!   atômica (AI_RULES.md §4.Transações).

pub mod model;
pub mod repository;
pub mod service;
