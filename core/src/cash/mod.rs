//! Gestão de caixa: sessões de abertura/fechamento e movimentações
//! (vendas, sangrias, suprimentos).
//!
//! Regras aplicadas (AI_RULES.md §1, §8):
//! - Domínio puro, sem dependência de UI/banco
//! - `CashSession` concentra ciclo de vida (Open → Closed)
//! - `CashMovement` é o livro-razão por sessão (movimentos imutáveis
//!   exceto sync flag) — base de auditoria e cálculo de totais.

pub mod model;
pub mod repository;
pub mod service;
