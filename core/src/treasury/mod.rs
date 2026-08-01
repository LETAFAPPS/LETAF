//! Carteira do estabelecimento (tesouraria) — saldo inicial da empresa.
//!
//! Regras aplicadas (AI_RULES.md §1, §6, §9):
//! - Domínio puro: model + repository (trait) + service.
//! - Entidade com `BaseFields` (UUID, company_id, soft delete, sync).
//! - SINGLETON por empresa: no máximo UMA carteira por `company_id`
//!   (unique na migration; service valida na criação).
//! - `initial_balance >= 0` e `reserve_goal >= 0` (meta de reserva
//!   mensal) — validados no service (§11: nunca confiar no frontend).
//! - Movimentos manuais (`TreasuryMovement`: aporte/retirada) são
//!   append-only, com `amount > 0` e direção pelo `kind`.

//! - A consolidação do caixa (o que é dinheiro NOVO entrando/saindo)
//!   vive em [`analytics`] — regra de negócio pura, fora da UI (§1/§3).

pub mod analytics;
pub mod model;
pub mod repository;
pub mod service;
