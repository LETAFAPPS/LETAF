use std::sync::Arc;

use chrono::Utc;
use uuid::Uuid;

use crate::error::CoreError;

use super::model::BusinessType;
use super::repository::BusinessTypeRepository;

/// Dados de entrada para criar/editar um tipo de empresa.
pub struct BusinessTypeInput {
    pub name: String,
    pub description: String,
    pub active: bool,
    pub sort_order: i32,
}

/// Regras do catálogo de tipos de empresa (gestão pelo super admin).
/// Valida entrada (§11) e delega ao repository (§10).
pub struct BusinessTypeService {
    repo: Arc<dyn BusinessTypeRepository>,
}

impl BusinessTypeService {
    pub fn new(repo: Arc<dyn BusinessTypeRepository>) -> Self {
        Self { repo }
    }

    pub async fn find_all(&self) -> Result<Vec<BusinessType>, CoreError> {
        self.repo.find_all().await
    }

    pub async fn find_active(&self) -> Result<Vec<BusinessType>, CoreError> {
        self.repo.find_active().await
    }

    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<BusinessType>, CoreError> {
        self.repo.find_by_id(id).await
    }

    pub async fn create(&self, input: BusinessTypeInput) -> Result<BusinessType, CoreError> {
        let item = build(Uuid::new_v4(), input)?;
        self.repo.create(&item).await?;
        Ok(item)
    }

    pub async fn update(&self, id: Uuid, input: BusinessTypeInput) -> Result<BusinessType, CoreError> {
        let existing = self
            .repo
            .find_by_id(id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Tipo de empresa não encontrado".into()))?;
        let mut item = build(id, input)?;
        item.created_at = existing.created_at; // preserva criação
        self.repo.update(&item).await?;
        Ok(item)
    }

    pub async fn soft_delete(&self, id: Uuid) -> Result<(), CoreError> {
        self.repo.soft_delete(id).await
    }
}

/// Valida e monta um `BusinessType` a partir da entrada.
fn build(id: Uuid, input: BusinessTypeInput) -> Result<BusinessType, CoreError> {
    if input.name.trim().is_empty() {
        return Err(CoreError::Validation("Informe o nome do tipo de empresa".into()));
    }
    let now = Utc::now().naive_utc();
    Ok(BusinessType {
        id,
        name: input.name.trim().to_string(),
        description: input.description.trim().to_string(),
        active: input.active,
        sort_order: input.sort_order,
        created_at: now,
        updated_at: now,
        deleted_at: None,
    })
}
