use async_trait::async_trait;
use uuid::Uuid;

use crate::error::CoreError;

use super::model::AdminRole;

/// Acesso a dados das Funções de administrador (§10 — só via repository).
/// Global (sem company_id); implementado no servidor (PostgreSQL).
#[async_trait]
pub trait AdminRoleRepository: Send + Sync {
    async fn find_all(&self) -> Result<Vec<AdminRole>, CoreError>;
    async fn find_by_id(&self, id: Uuid) -> Result<Option<AdminRole>, CoreError>;
    async fn create(&self, role: &AdminRole) -> Result<(), CoreError>;
    async fn update(&self, role: &AdminRole) -> Result<(), CoreError>;
    async fn soft_delete(&self, id: Uuid) -> Result<(), CoreError>;

    /// Quantos usuários estão atribuídos a esta função.
    async fn count_users(&self, role_id: Uuid) -> Result<i64, CoreError>;
    /// Define (ou remove, com `None`) a função de um usuário.
    async fn set_user_role(&self, user_id: Uuid, role_id: Option<Uuid>) -> Result<(), CoreError>;
    /// Função atribuída a um usuário (`None` = master/sem restrição).
    async fn role_for_user(&self, user_id: Uuid) -> Result<Option<AdminRole>, CoreError>;
    /// Id da função de cada usuário informado (para listar sem N+1).
    async fn roles_of_users(&self, user_ids: &[Uuid]) -> Result<Vec<(Uuid, Uuid)>, CoreError>;

    /// Ativa/desativa o acesso de um admin (só tem efeito em admins com
    /// função atribuída; o master não tem linha e é sempre ativo).
    async fn set_user_active(&self, user_id: Uuid, active: bool) -> Result<(), CoreError>;
    /// `true` se o admin pode logar (sem linha = ativo).
    async fn is_user_active(&self, user_id: Uuid) -> Result<bool, CoreError>;
    /// `active` de cada usuário informado (ausente = ativo).
    async fn active_of_users(&self, user_ids: &[Uuid]) -> Result<Vec<(Uuid, bool)>, CoreError>;
}
