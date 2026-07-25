//! Funções de administrador (RBAC do PAINEL do super admin).
//!
//! Nível PLATAFORMA (global, sem `company_id`) — exceção documentada ao
//! multi-tenant, igual ao super admin/planos. Cada Função define quais TELAS
//! do painel um usuário acessa; a autoridade é o backend (§11), que valida a
//! tela em cada rota `/admin/*`. Módulo `model`/`service`/`repository` (§9).
pub mod model;
pub mod repository;
pub mod service;
