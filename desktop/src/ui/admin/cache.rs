//! Caches em memória das listas do painel.
//!
//! Listas COMPLETAS vindas da API; a busca/filtro é aplicada sobre elas
//! para montar o modelo exibido (§13 — sem novo round-trip por tecla).

use std::sync::{Arc, Mutex};

use super::dto::{
    AdminDto, AdminRoleDto, BusinessTypeDto, CompanyDto, PlanDto, SubscriptionDto,
};

/// Cache dos planos crus (para o "editar" preencher o form com os valores
/// numéricos, já que o modelo Slint só guarda os textos de exibição).
pub(super) type PlansCache = Arc<Mutex<Vec<PlanDto>>>;
/// Cache dos tipos de empresa crus (para o "editar" preencher o form).
pub(super) type BusinessTypesCache = Arc<Mutex<Vec<BusinessTypeDto>>>;
pub(super) type CompaniesCache = Arc<Mutex<Vec<CompanyDto>>>;
pub(super) type SubsCache = Arc<Mutex<Vec<SubscriptionDto>>>;
pub(super) type UsersCache = Arc<Mutex<Vec<AdminDto>>>;
pub(super) type RolesCache = Arc<Mutex<Vec<AdminRoleDto>>>;

/// Conjunto de caches do painel: criado uma vez em `setup_admin` e
/// compartilhado (clone dos `Arc`) entre refresh, filtros e formulários.
#[derive(Clone, Default)]
pub(super) struct AdminCaches {
    pub(super) plans: PlansCache,
    pub(super) business_types: BusinessTypesCache,
    pub(super) companies: CompaniesCache,
    pub(super) subs: SubsCache,
    pub(super) users: UsersCache,
    pub(super) roles: RolesCache,
}
