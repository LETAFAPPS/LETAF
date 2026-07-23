//! Trilha de auditoria das ações do super admin (§11).
//!
//! Nível PLATAFORMA (global, sem `company_id`) — exceção documentada ao
//! multi-tenant, igual ao catálogo de planos. Registra QUEM fez O QUÊ e
//! QUANDO nas rotas `/admin/*`, para rastreabilidade de ações sensíveis
//! (suspender/excluir empresa, mexer em assinatura, baixar fatura...).
//!
//! Só existe no servidor: não sincroniza com o desktop.
pub mod model;
pub mod repository;
pub mod service;
