//! Crate `letaf-core` — domínio puro do ERP.
//!
//! Regras aplicadas (AI_RULES.md §1, §6, §8):
//! - Domínio não depende de UI nem de banco
//! - Entidades carregam BaseFields (id, company_id, timestamps, soft delete, synced)
//!
//! `clippy::too_many_arguments` está silenciado porque vários construtores e
//! métodos de service recebem os campos do domínio diretamente (ex: Product
//! com 11 campos). Refatorar para DTOs é mudança maior sem ganho funcional —
//! deixar como está mantém legibilidade e tipagem forte.
#![allow(clippy::too_many_arguments)]

pub mod addon;
pub mod addon_group;
pub mod auth;
pub mod availability;
pub mod banner;
pub mod business_hours;
pub mod cash;
pub mod category;
pub mod company;
pub mod coupon;
pub mod customer;
pub mod customer_address;
pub mod dashboard;
pub mod discount;
pub mod entity;
pub mod error;
pub mod finance;
pub mod finance_category;
#[cfg(feature = "password-hashing")]
pub mod hashing;
pub mod job_role;
pub mod money;
pub mod order;
pub mod password_reset;
pub mod plan;
pub mod payment_gateway;
pub mod payment_method;
pub mod permission;
pub mod printer;
pub mod product;
pub mod reconcile;
pub mod subcategory;
pub mod subscription;
pub mod util;
pub mod wallet;
