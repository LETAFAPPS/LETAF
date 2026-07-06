//! Tela de Colaboradores (RBAC): Funções (cargos + permissões) e
//! Funcionários (login + Função). Offline-first; o backend é a autoridade.

mod render;
mod setup;

pub(crate) use setup::setup_collaborators;
