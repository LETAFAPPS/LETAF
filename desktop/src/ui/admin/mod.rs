//! Painel do administrador (super admin) — camada de UI do desktop.
//!
//! Regras (AI_RULES.md §1, §3, §11):
//! - UI burra: aqui só chamamos as rotas `/admin/*` (online) e refletimos
//!   o resultado nos modelos Slint. A autoridade é o backend, que exige
//!   `role == super_admin` em toda rota.
//! - Diferente das telas da loja (offline-first/SQLite), o painel é
//!   inerentemente ONLINE: o super admin gere dados cross-tenant do
//!   servidor, não há espelho local.
//!
//! Dividido por responsabilidade (§8, §9) — o arquivo único anterior havia
//! crescido demais:
//! - `dto`: DTOs das respostas JSON do servidor
//! - `cache`: listas completas em memória, base dos filtros
//! - `format`: formatação/parse dos valores exibidos
//! - `http`: GET/escrita autenticados e feedback padrão das operações
//! - `filters`: busca/filtro das listas e montagem das linhas exibidas
//! - `refresh`: recarga de todos os dados do painel
//! - `roles`: funções (telas permitidas) do painel
//! - `users`: administradores do painel
//! - `companies`: cadastro/edição/suspensão/exclusão e detalhe das empresas
//! - `company_form`: preenchimento, limpeza, imagens e máscaras do cadastro
//! - `subscriptions`: assinatura e faturas de uma empresa
//! - `plans`: catálogo de planos
//! - `business_types`: catálogo de tipos de empresa
//! - `geo`: preenchimento de endereço por CEP (ViaCEP)
//! - `impersonation`: entrar numa empresa e voltar ao super admin

mod business_types;
mod cache;
mod companies;
mod company_form;
mod dto;
mod filters;
mod format;
mod geo;
mod http;
mod impersonation;
mod plans;
mod refresh;
mod roles;
mod subscriptions;
mod users;

use std::sync::Arc;

use tokio::sync::{Notify, RwLock};

use crate::context::DesktopState;
use crate::MainWindow;

use self::cache::AdminCaches;

/// Registra todos os callbacks do painel do administrador.
pub(crate) fn setup_admin(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
    auth_token: Arc<RwLock<Option<String>>>,
    server_url: String,
) {
    let caches = AdminCaches::default();
    refresh::setup_refresh(ui, handle, &auth_token, &server_url, &caches);
    filters::setup_filters(ui, &caches);
    roles::setup_roles(ui, handle, &auth_token, &server_url, &caches.roles);
    users::setup_form(ui);
    users::setup_persist(ui, handle, &auth_token, &server_url);
    companies::setup_companies(ui, handle, &auth_token, &server_url);
    subscriptions::setup_subscriptions(ui, handle, &auth_token, &server_url);
    company_form::setup_company_pickers(ui, handle);
    company_form::setup_company_form_helpers(ui);
    geo::setup_company_cep(ui, handle);
    plans::setup_plan_form(ui, &caches.plans);
    plans::setup_plan_persist(ui, handle, &auth_token, &server_url);
    business_types::setup_business_type_form(ui, &caches.business_types);
    business_types::setup_business_type_persist(ui, handle, &auth_token, &server_url);
    impersonation::setup_impersonation(ui, state, handle, sync_notify, &auth_token, &server_url);
}
