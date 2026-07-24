use std::fmt;

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entity::BaseFields;

/// Nível de acesso de um usuário operador.
///
/// Regras aplicadas (AI_RULES.md §11):
/// - `SuperAdmin`: escopo cross-tenant — gestão de empresas, planos, relatórios.
///   Não está vinculado a `company_id` específico (uso futuro — Fase 2).
/// - `Admin`: dono do estabelecimento; gestão completa do tenant.
/// - `Employee`: colaborador; gestão do tenant com restrições futuras
///   (capabilities granulares definidas pelo Admin).
///
/// Cliente final (`customer`) vive na entidade `Customer`, fora deste enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum UserRole {
    SuperAdmin,
    #[default]
    Admin,
    Employee,
}

impl UserRole {
    /// Representação serializável no banco e no JWT.
    pub fn as_db_str(&self) -> &'static str {
        match self {
            Self::SuperAdmin => "super_admin",
            Self::Admin      => "admin",
            Self::Employee   => "employee",
        }
    }

    /// Decodifica a representação do banco para `UserRole`.
    /// Retorna `None` para valores desconhecidos (caller decide o fallback).
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "super_admin" => Some(Self::SuperAdmin),
            "admin"       => Some(Self::Admin),
            "employee"    => Some(Self::Employee),
            _ => None,
        }
    }

    /// Rótulo curto em pt-BR para exibição em UI.
    pub fn label_pt_br(&self) -> &'static str {
        match self {
            Self::SuperAdmin => "Super Admin",
            Self::Admin      => "Admin",
            Self::Employee   => "Funcionário",
        }
    }

    /// `true` para papéis com acesso total (bypass do RBAC — §11).
    /// Espelha `AuthClaims::require_permission` no servidor.
    pub fn is_admin(&self) -> bool {
        matches!(self, Self::Admin | Self::SuperAdmin)
    }

    /// `true` apenas para o super admin de plataforma (cross-tenant).
    /// Usado para rotear o painel de administrador (menus próprios) e
    /// liberar as rotas `/admin/*` — a autoridade é sempre o backend (§11).
    pub fn is_super_admin(&self) -> bool {
        matches!(self, Self::SuperAdmin)
    }
}

impl fmt::Display for UserRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_db_str())
    }
}

/// Entidade User — base para autenticação.
///
/// Regras aplicadas (AI_RULES.md §6, §11):
/// - Campos base obrigatórios (UUID, company_id, timestamps, synced)
/// - Preparar autenticação (JWT ou similar)
///
/// Cada usuário pertence a uma empresa (company_id via BaseFields).
/// `role` controla o nível de acesso operacional.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    #[serde(flatten)]
    pub base: BaseFields,
    pub email: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub name: String,
    #[serde(default)]
    pub role: UserRole,
    /// Função (cargo) atribuída — define as permissões do `Employee`.
    /// `None` para Admin/SuperAdmin (acesso total) ou funcionário sem
    /// função. Ver [`crate::job_role`] e [`crate::permission`].
    #[serde(default)]
    pub job_role_id: Option<Uuid>,
    /// Foto de perfil (imagem JPEG/PNG em base64). `None` = sem foto.
    /// Editável pelo próprio operador via `PUT /auth/profile`.
    #[serde(default)]
    pub avatar: Option<String>,
    /// Telefone de contato do operador (proprietário/admin). `None` = sem
    /// telefone. Exibido no painel do super admin.
    #[serde(default)]
    pub phone: Option<String>,
}

impl User {
    pub fn new(
        company_id: Uuid,
        email: String,
        password_hash: String,
        name: String,
        role: UserRole,
    ) -> Self {
        Self {
            base: BaseFields::new(company_id),
            email,
            password_hash,
            name,
            role,
            job_role_id: None,
            avatar: None,
            phone: None,
        }
    }
}

/// Payload de sincronização para User.
///
/// Regras aplicadas (AI_RULES.md §7, §11):
/// - Inclui password_hash (necessário para upsert no servidor)
/// - User.password_hash tem skip_serializing (segurança em APIs),
///   mas a sync precisa transmitir o hash para replicação.
/// - `role` propagado entre desktop ↔ servidor.
#[derive(Debug, Serialize, Deserialize)]
pub struct SyncUserPayload {
    pub id: Uuid,
    pub company_id: Uuid,
    pub email: String,
    pub password_hash: String,
    pub name: String,
    #[serde(default)]
    pub role: UserRole,
    #[serde(default)]
    pub job_role_id: Option<Uuid>,
    #[serde(default)]
    pub avatar: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub deleted_at: Option<NaiveDateTime>,
    pub synced: bool,
}

impl From<&User> for SyncUserPayload {
    fn from(u: &User) -> Self {
        Self {
            id: u.base.id,
            company_id: u.base.company_id,
            email: u.email.clone(),
            password_hash: u.password_hash.clone(),
            name: u.name.clone(),
            role: u.role,
            job_role_id: u.job_role_id,
            avatar: u.avatar.clone(),
            phone: u.phone.clone(),
            created_at: u.base.created_at,
            updated_at: u.base.updated_at,
            deleted_at: u.base.deleted_at,
            synced: u.base.synced,
        }
    }
}

impl SyncUserPayload {
    /// Converte o payload em entidade User.
    pub fn into_user(self) -> User {
        User {
            base: BaseFields {
                id: self.id,
                company_id: self.company_id,
                created_at: self.created_at,
                updated_at: self.updated_at,
                deleted_at: self.deleted_at,
                synced: self.synced,
            },
            email: self.email,
            password_hash: self.password_hash,
            name: self.name,
            role: self.role,
            job_role_id: self.job_role_id,
            avatar: self.avatar,
            phone: self.phone,
        }
    }
}
