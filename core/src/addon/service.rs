use std::sync::Arc;
use rust_decimal::Decimal;

use uuid::Uuid;

use super::model::Addon;
use super::repository::AddonRepository;
use crate::addon_group::repository::AddonGroupRepository;
use crate::error::CoreError;

/// Service para o domínio Addon.
///
/// Regras aplicadas (AI_RULES.md §1, §9, §11):
/// - Validação de `group_id` pertencente à mesma empresa (§11).
/// - Toda escrita marca `synced = false` para o sync worker (§7).
pub struct AddonService {
    repo: Arc<dyn AddonRepository>,
    group_repo: Arc<dyn AddonGroupRepository>,
}

impl AddonService {
    pub fn new(
        repo: Arc<dyn AddonRepository>,
        group_repo: Arc<dyn AddonGroupRepository>,
    ) -> Self {
        Self { repo, group_repo }
    }

    pub async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Addon>, CoreError> {
        self.repo.find_by_id(company_id, id).await
    }

    pub async fn find_all(&self, company_id: Uuid) -> Result<Vec<Addon>, CoreError> {
        self.repo.find_all(company_id).await
    }

    pub async fn find_by_group(&self, company_id: Uuid, group_id: Uuid) -> Result<Vec<Addon>, CoreError> {
        self.repo.find_by_group(company_id, group_id).await
    }

    pub async fn create(
        &self,
        company_id: Uuid,
        group_id: Uuid,
        name: String,
        price: Decimal,
    ) -> Result<Addon, CoreError> {
        Self::validate(&name, price)?;
        self.ensure_group_belongs_to_company(company_id, group_id).await?;
        let addon = Addon::new(company_id, group_id, name, price);
        self.repo.create(&addon).await?;
        Ok(addon)
    }

    pub async fn update(
        &self,
        company_id: Uuid,
        id: Uuid,
        group_id: Uuid,
        name: String,
        price: Decimal,
    ) -> Result<Addon, CoreError> {
        Self::validate(&name, price)?;
        self.ensure_group_belongs_to_company(company_id, group_id).await?;
        let mut addon = self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("Addon not found".into()))?;
        addon.group_id = group_id;
        addon.name = name;
        addon.price = price;
        addon.base.updated_at = chrono::Utc::now().naive_utc();
        addon.base.synced = false;
        self.repo.update(&addon).await?;
        Ok(addon)
    }

    pub async fn update_sort_order(
        &self,
        company_id: Uuid,
        id: Uuid,
        sort_order: i32,
    ) -> Result<(), CoreError> {
        let mut a = self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("Addon not found".into()))?;
        a.sort_order = sort_order;
        a.base.updated_at = chrono::Utc::now().naive_utc();
        a.base.synced = false;
        self.repo.update(&a).await
    }

    pub async fn toggle_active(&self, company_id: Uuid, id: Uuid, active: bool) -> Result<(), CoreError> {
        let mut a = self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("Addon not found".into()))?;
        a.active = active;
        a.base.updated_at = chrono::Utc::now().naive_utc();
        a.base.synced = false;
        self.repo.update(&a).await
    }

    pub async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("Addon not found".into()))?;
        self.repo.soft_delete(company_id, id).await
    }

    pub async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Addon>, CoreError> {
        self.repo.find_unsynced(company_id).await
    }

    pub async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        self.repo.mark_synced(company_id, id, updated_at).await
    }

    pub async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
    ) -> Result<Vec<Addon>, CoreError> {
        self.repo.find_updated_since(company_id, since).await
    }

    /// Upsert de sincronização (§7.7 — last-write-wins).
    pub async fn sync_upsert(
        &self,
        company_id: Uuid,
        mut addon: Addon,
    ) -> Result<(), CoreError> {
        if addon.base.company_id != company_id {
            return Err(CoreError::Validation("Company mismatch".into()));
        }
        addon.base.synced = true;
        self.repo.sync_upsert(&addon).await
    }

    fn validate(name: &str, price: Decimal) -> Result<(), CoreError> {
        if name.trim().is_empty() {
            return Err(CoreError::Validation("Addon name is required".into()));
        }
        if price < Decimal::ZERO {
            return Err(CoreError::Validation("Addon price cannot be negative".into()));
        }
        Ok(())
    }

    /// Valida que o grupo existe e pertence à mesma empresa (§11).
    async fn ensure_group_belongs_to_company(
        &self,
        company_id: Uuid,
        group_id: Uuid,
    ) -> Result<(), CoreError> {
        self.group_repo
            .find_by_id(company_id, group_id)
            .await?
            .ok_or_else(|| CoreError::Validation("Invalid group_id".into()))?;
        Ok(())
    }
}
