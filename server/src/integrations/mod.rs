//! Integrações com serviços externos.
//!
//! Regras aplicadas (AI_RULES.md §1, §11):
//! - Implementações concretas de traits do core.
//! - Toda chamada HTTP fica aqui — o core continua agnóstico.
//! - Credenciais lidas de `AppConfig`, nunca hardcoded.

pub mod efi;
