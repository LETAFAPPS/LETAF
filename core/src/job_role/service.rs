use std::sync::Arc;

use uuid::Uuid;

use super::model::JobRole;
use super::repository::JobRoleRepository;
use crate::error::CoreError;
use crate::permission;

/// Valida o nome e o conjunto de permissões de uma função.
///
/// AI_RULES §11 (nunca confiar no frontend): cada permissão precisa
/// existir no catálogo [`crate::permission`]; chaves desconhecidas são
/// rejeitadas. Duplicatas são removidas.
fn sanitize(name: &str, permissions: Vec<String>) -> Result<Vec<String>, CoreError> {
    if name.trim().is_empty() {
        return Err(CoreError::Validation("Nome da função é obrigatório".into()));
    }
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for p in permissions {
        if !permission::is_valid(&p) {
            return Err(CoreError::Validation(format!("Permissão inválida: '{p}'")));
        }
        if seen.insert(p.clone()) {
            out.push(p);
        }
    }
    Ok(out)
}

/// Service do domínio JobRole (função/cargo).
///
/// Regras (AI_RULES §1, §9, §11): orquestra regra de negócio; depende do
/// repository via trait; valida toda entrada no backend.
pub struct JobRoleService {
    repo: Arc<dyn JobRoleRepository>,
}

impl JobRoleService {
    pub fn new(repo: Arc<dyn JobRoleRepository>) -> Self {
        Self { repo }
    }

    pub async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<JobRole>, CoreError> {
        self.repo.find_by_id(company_id, id).await
    }

    pub async fn find_all(&self, company_id: Uuid) -> Result<Vec<JobRole>, CoreError> {
        self.repo.find_all(company_id).await
    }

    pub async fn create(
        &self,
        company_id: Uuid,
        name: String,
        permissions: Vec<String>,
    ) -> Result<JobRole, CoreError> {
        let permissions = sanitize(&name, permissions)?;
        let role = JobRole::new(company_id, name.trim().to_string(), permissions);
        self.repo.create(&role).await?;
        Ok(role)
    }

    pub async fn update(
        &self,
        company_id: Uuid,
        id: Uuid,
        name: String,
        permissions: Vec<String>,
    ) -> Result<JobRole, CoreError> {
        let permissions = sanitize(&name, permissions)?;
        let mut role = self
            .repo
            .find_by_id(company_id, id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Função não encontrada".into()))?;
        role.name = name.trim().to_string();
        role.permissions = permissions;
        role.base.updated_at = chrono::Utc::now().naive_utc();
        role.base.synced = false;
        self.repo.update(&role).await?;
        Ok(role)
    }

    pub async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        self.repo
            .find_by_id(company_id, id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Função não encontrada".into()))?;
        self.repo.soft_delete(company_id, id).await
    }

    pub async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<JobRole>, CoreError> {
        self.repo.find_unsynced(company_id).await
    }

    pub async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        self.repo.mark_synced(company_id, id, updated_at).await
    }

    pub async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
    ) -> Result<Vec<JobRole>, CoreError> {
        self.repo.find_updated_since(company_id, since).await
    }

    /// Upsert de sincronização (§7.7 — last-write-wins).
    pub async fn sync_upsert(&self, company_id: Uuid, mut role: JobRole) -> Result<(), CoreError> {
        if role.base.company_id != company_id {
            return Err(CoreError::Validation("Company mismatch".into()));
        }
        role.base.synced = true;
        self.repo.sync_upsert(&role).await
    }
}
