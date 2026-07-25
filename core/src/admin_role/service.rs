use std::sync::Arc;

use chrono::Utc;
use uuid::Uuid;

use crate::error::CoreError;

use super::model::{screen_is_valid, AdminRole};
use super::repository::AdminRoleRepository;

/// Regras das Funções de administrador (gestão pelo super admin). Valida a
/// entrada (§11 — nunca confiar no frontend) e delega ao repository (§10).
pub struct AdminRoleService {
    repo: Arc<dyn AdminRoleRepository>,
}

impl AdminRoleService {
    pub fn new(repo: Arc<dyn AdminRoleRepository>) -> Self {
        Self { repo }
    }

    pub async fn find_all(&self) -> Result<Vec<AdminRole>, CoreError> {
        self.repo.find_all().await
    }

    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<AdminRole>, CoreError> {
        self.repo.find_by_id(id).await
    }

    pub async fn create(&self, name: String, screens: Vec<String>) -> Result<AdminRole, CoreError> {
        let role = build(Uuid::new_v4(), name, screens)?;
        self.repo.create(&role).await?;
        Ok(role)
    }

    pub async fn update(&self, id: Uuid, name: String, screens: Vec<String>) -> Result<AdminRole, CoreError> {
        let existing = self
            .repo
            .find_by_id(id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Função não encontrada".into()))?;
        let mut role = build(id, name, screens)?;
        role.created_at = existing.created_at;
        self.repo.update(&role).await?;
        Ok(role)
    }

    pub async fn soft_delete(&self, id: Uuid) -> Result<(), CoreError> {
        self.repo.soft_delete(id).await
    }

    pub async fn count_users(&self, role_id: Uuid) -> Result<i64, CoreError> {
        self.repo.count_users(role_id).await
    }

    pub async fn set_user_role(&self, user_id: Uuid, role_id: Option<Uuid>) -> Result<(), CoreError> {
        self.repo.set_user_role(user_id, role_id).await
    }

    pub async fn role_for_user(&self, user_id: Uuid) -> Result<Option<AdminRole>, CoreError> {
        self.repo.role_for_user(user_id).await
    }

    pub async fn roles_of_users(&self, user_ids: &[Uuid]) -> Result<Vec<(Uuid, Uuid)>, CoreError> {
        self.repo.roles_of_users(user_ids).await
    }

    pub async fn set_user_active(&self, user_id: Uuid, active: bool) -> Result<(), CoreError> {
        self.repo.set_user_active(user_id, active).await
    }

    pub async fn is_user_active(&self, user_id: Uuid) -> Result<bool, CoreError> {
        self.repo.is_user_active(user_id).await
    }

    pub async fn active_of_users(&self, user_ids: &[Uuid]) -> Result<Vec<(Uuid, bool)>, CoreError> {
        self.repo.active_of_users(user_ids).await
    }
}

/// Valida e monta uma `AdminRole` a partir da entrada.
fn build(id: Uuid, name: String, mut screens: Vec<String>) -> Result<AdminRole, CoreError> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(CoreError::Validation("Informe o nome da função".into()));
    }
    screens.retain(|s| !s.trim().is_empty());
    for s in &screens {
        if !screen_is_valid(s) {
            return Err(CoreError::Validation(format!("Tela inválida: '{s}'")));
        }
    }
    if screens.is_empty() {
        return Err(CoreError::Validation("Selecione ao menos uma tela".into()));
    }
    let now = Utc::now().naive_utc();
    Ok(AdminRole {
        id,
        name,
        screens,
        created_at: now,
        updated_at: now,
        deleted_at: None,
    })
}
