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
}
