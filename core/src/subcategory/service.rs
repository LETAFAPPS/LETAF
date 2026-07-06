use std::sync::Arc;

use uuid::Uuid;

use super::model::Subcategory;
use super::repository::SubcategoryRepository;
use crate::category::repository::CategoryRepository;
use crate::error::CoreError;

/// Service para o domínio Subcategory.
///
/// Regras aplicadas (AI_RULES.md §1, §9, §11):
/// - Orquestração de regras de negócio (sem SQL).
/// - Depende dos repositories via trait (inversão de dependência).
/// - Validação de `category_id` pertencente à mesma empresa (§11).
pub struct SubcategoryService {
    repo: Arc<dyn SubcategoryRepository>,
    category_repo: Arc<dyn CategoryRepository>,
}

impl SubcategoryService {
    pub fn new(
        repo: Arc<dyn SubcategoryRepository>,
        category_repo: Arc<dyn CategoryRepository>,
    ) -> Self {
        Self { repo, category_repo }
    }

    pub async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Subcategory>, CoreError> {
        self.repo.find_by_id(company_id, id).await
    }

    pub async fn find_all(&self, company_id: Uuid) -> Result<Vec<Subcategory>, CoreError> {
        self.repo.find_all(company_id).await
    }

    pub async fn find_by_category(&self, company_id: Uuid, category_id: Uuid) -> Result<Vec<Subcategory>, CoreError> {
        self.repo.find_by_category(company_id, category_id).await
    }

    /// Cria uma subcategoria a partir de dados brutos.
    ///
    /// Regras aplicadas (AI_RULES.md §11):
    /// - Valida que `category_id` pertence à mesma empresa (evita
    ///   vinculação cross-tenant).
    pub async fn create(
        &self,
        company_id: Uuid,
        category_id: Uuid,
        name: String,
    ) -> Result<Subcategory, CoreError> {
        if name.trim().is_empty() {
            return Err(CoreError::Validation("Subcategory name is required".into()));
        }
        self.ensure_category_belongs_to_company(company_id, category_id).await?;

        let subcategory = Subcategory::new(company_id, category_id, name);
        self.repo.create(&subcategory).await?;
        Ok(subcategory)
    }

    /// Atualiza uma subcategoria existente.
    pub async fn update(
        &self,
        company_id: Uuid,
        id: Uuid,
        category_id: Uuid,
        name: String,
    ) -> Result<Subcategory, CoreError> {
        if name.trim().is_empty() {
            return Err(CoreError::Validation("Subcategory name is required".into()));
        }
        self.ensure_category_belongs_to_company(company_id, category_id).await?;

        let mut subcategory = self
            .repo
            .find_by_id(company_id, id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Subcategory not found".into()))?;

        subcategory.category_id = category_id;
        subcategory.name = name;
        subcategory.base.updated_at = chrono::Utc::now().naive_utc();
        subcategory.base.synced = false;

        self.repo.update(&subcategory).await?;
        Ok(subcategory)
    }

    /// Atualiza apenas a ordem de exibição de uma subcategoria.
    pub async fn update_sort_order(
        &self,
        company_id: Uuid,
        id: Uuid,
        sort_order: i32,
    ) -> Result<(), CoreError> {
        let mut sub = self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("Subcategory not found".into()))?;
        sub.sort_order = sort_order;
        sub.base.updated_at = chrono::Utc::now().naive_utc();
        sub.base.synced = false;
        self.repo.update(&sub).await
    }

    /// Remoção lógica (soft delete).
    pub async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        self.repo
            .find_by_id(company_id, id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Subcategory not found".into()))?;
        self.repo.soft_delete(company_id, id).await
    }

    pub async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Subcategory>, CoreError> {
        self.repo.find_unsynced(company_id).await
    }

    pub async fn mark_synced(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        self.repo.mark_synced(company_id, id).await
    }

    pub async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
    ) -> Result<Vec<Subcategory>, CoreError> {
        self.repo.find_updated_since(company_id, since).await
    }

    /// Upsert de sincronização (§7.7 — last-write-wins).
    pub async fn sync_upsert(
        &self,
        company_id: Uuid,
        mut subcategory: Subcategory,
    ) -> Result<(), CoreError> {
        if subcategory.base.company_id != company_id {
            return Err(CoreError::Validation("Company mismatch".into()));
        }
        subcategory.base.synced = true;
        self.repo.sync_upsert(&subcategory).await
    }

    /// Valida que a categoria existe e pertence à mesma empresa (§11).
    async fn ensure_category_belongs_to_company(
        &self,
        company_id: Uuid,
        category_id: Uuid,
    ) -> Result<(), CoreError> {
        self.category_repo
            .find_by_id(company_id, category_id)
            .await?
            .ok_or_else(|| CoreError::Validation("Invalid category_id".into()))?;
        Ok(())
    }
}
