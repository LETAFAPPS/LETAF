use std::sync::Arc;

use uuid::Uuid;

use super::icons;
use super::model::Category;
use super::repository::CategoryRepository;
use crate::error::CoreError;

/// Normaliza e valida o slug do ícone. `Some("")` ou apenas espaços
/// vira `None`; slug fora da allowlist retorna erro de validação.
fn sanitize_icon_name(raw: Option<String>) -> Result<Option<String>, CoreError> {
    let trimmed = raw.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty());
    match trimmed {
        None => Ok(None),
        Some(slug) if icons::is_valid(slug) => Ok(Some(slug.to_string())),
        Some(slug) => Err(CoreError::Validation(format!(
            "Unknown category icon '{slug}'"
        ))),
    }
}

/// Service para o domínio Category.
///
/// Regras aplicadas (AI_RULES.md §1, §9, §11):
/// - service.rs contém a orquestração de regras de negócio
/// - Depende de repository via trait (inversão de dependência)
/// - Validar todos os dados de entrada no backend
pub struct CategoryService {
    repo: Arc<dyn CategoryRepository>,
}

impl CategoryService {
    pub fn new(repo: Arc<dyn CategoryRepository>) -> Self {
        Self { repo }
    }

    pub async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Category>, CoreError> {
        self.repo.find_by_id(company_id, id).await
    }

    pub async fn find_all(&self, company_id: Uuid) -> Result<Vec<Category>, CoreError> {
        self.repo.find_all(company_id).await
    }

    /// Cria uma categoria a partir de dados brutos.
    ///
    /// `icon_name` é validado contra a allowlist `category::icons::ICONS`
    /// (AI_RULES §11 — nunca confiar no frontend). Slugs inválidos
    /// retornam erro de validação. `None` = categoria sem ícone.
    pub async fn create(
        &self,
        company_id: Uuid,
        name: String,
        description: Option<String>,
        icon_name: Option<String>,
    ) -> Result<Category, CoreError> {
        if name.trim().is_empty() {
            return Err(CoreError::Validation("Category name is required".into()));
        }
        let icon_name = sanitize_icon_name(icon_name)?;
        let mut category = Category::new(company_id, name, description);
        category.icon_name = icon_name;
        self.repo.create(&category).await?;
        Ok(category)
    }

    /// Atualiza uma categoria existente.
    pub async fn update(
        &self,
        company_id: Uuid,
        id: Uuid,
        name: String,
        description: Option<String>,
        icon_name: Option<String>,
    ) -> Result<Category, CoreError> {
        if name.trim().is_empty() {
            return Err(CoreError::Validation("Category name is required".into()));
        }
        let icon_name = sanitize_icon_name(icon_name)?;
        let mut category = self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("Category not found".into()))?;

        category.name = name;
        category.description = description;
        category.icon_name = icon_name;
        category.base.updated_at = chrono::Utc::now().naive_utc();
        category.base.synced = false;

        self.repo.update(&category).await?;
        Ok(category)
    }

    /// Atualiza apenas a ordem de exibição de uma categoria.
    pub async fn update_sort_order(
        &self,
        company_id: Uuid,
        id: Uuid,
        sort_order: i32,
    ) -> Result<(), CoreError> {
        let mut cat = self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("Category not found".into()))?;
        cat.sort_order = sort_order;
        cat.base.updated_at = chrono::Utc::now().naive_utc();
        cat.base.synced = false;
        self.repo.update(&cat).await
    }

    /// Remoção lógica (soft delete).
    pub async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("Category not found".into()))?;
        self.repo.soft_delete(company_id, id).await
    }

    /// Busca categorias ainda não sincronizadas (§7).
    pub async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Category>, CoreError> {
        self.repo.find_unsynced(company_id).await
    }

    /// Marca categoria como sincronizada (§7).
    pub async fn mark_synced(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        self.repo.mark_synced(company_id, id).await
    }

    /// Busca categorias atualizadas após o timestamp (§7 — sync pull).
    pub async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
    ) -> Result<Vec<Category>, CoreError> {
        self.repo.find_updated_since(company_id, since).await
    }

    /// Upsert de sincronização (§7.7 — last-write-wins).
    pub async fn sync_upsert(
        &self,
        company_id: Uuid,
        mut category: Category,
    ) -> Result<(), CoreError> {
        if category.base.company_id != company_id {
            return Err(CoreError::Validation("Company mismatch".into()));
        }
        category.base.synced = true;
        self.repo.sync_upsert(&category).await
    }
}
