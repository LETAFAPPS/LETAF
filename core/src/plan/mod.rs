//! Catálogo de planos de assinatura (gerido pelo super admin).
//!
//! Nível PLATAFORMA (global, sem `company_id`) — exceção documentada ao
//! multi-tenant, igual ao super admin. Módulo `model`/`service`/`repository`
//! (§9), acesso a dados só via repository (§10).
pub mod model;
pub mod repository;
pub mod service;
