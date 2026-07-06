use std::sync::Arc;

use uuid::Uuid;

use super::model::AddonGroup;
use super::repository::AddonGroupRepository;
use crate::error::CoreError;

/// Service para o domínio AddonGroup.
///
/// Regras aplicadas (AI_RULES.md §1, §9, §11):
/// - Orquestração de regras de negócio (sem SQL).
/// - Depende do repository via trait (inversão de dependência).
/// - Validação centralizada de `selection` e `min/max` (§11).
pub struct AddonGroupService {
    repo: Arc<dyn AddonGroupRepository>,
}

impl AddonGroupService {
    pub fn new(repo: Arc<dyn AddonGroupRepository>) -> Self {
        Self { repo }
    }

    pub async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<AddonGroup>, CoreError> {
        self.repo.find_by_id(company_id, id).await
    }

    pub async fn find_all(&self, company_id: Uuid) -> Result<Vec<AddonGroup>, CoreError> {
        self.repo.find_all(company_id).await
    }

    pub async fn find_by_product(&self, company_id: Uuid, product_id: Uuid) -> Result<Vec<AddonGroup>, CoreError> {
        self.repo.find_by_product(company_id, product_id).await
    }

    pub async fn create(
        &self,
        company_id: Uuid,
        name: String,
        selection: String,
        min_select: i32,
        max_select: i32,
    ) -> Result<AddonGroup, CoreError> {
        let (min_select, max_select) = Self::normalize(&selection, min_select, max_select);
        Self::validate(&name, &selection, min_select, max_select)?;
        let group = AddonGroup::new(company_id, name, selection, min_select, max_select);
        self.repo.create(&group).await?;
        Ok(group)
    }

    pub async fn update(
        &self,
        company_id: Uuid,
        id: Uuid,
        name: String,
        selection: String,
        min_select: i32,
        max_select: i32,
    ) -> Result<AddonGroup, CoreError> {
        let (min_select, max_select) = Self::normalize(&selection, min_select, max_select);
        Self::validate(&name, &selection, min_select, max_select)?;
        let mut group = self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("AddonGroup not found".into()))?;
        group.name = name;
        group.selection = selection;
        group.min_select = min_select;
        group.max_select = max_select;
        group.base.updated_at = chrono::Utc::now().naive_utc();
        group.base.synced = false;
        self.repo.update(&group).await?;
        Ok(group)
    }

    pub async fn update_sort_order(
        &self,
        company_id: Uuid,
        id: Uuid,
        sort_order: i32,
    ) -> Result<(), CoreError> {
        let mut group = self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("AddonGroup not found".into()))?;
        group.sort_order = sort_order;
        group.base.updated_at = chrono::Utc::now().naive_utc();
        group.base.synced = false;
        self.repo.update(&group).await
    }

    pub async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("AddonGroup not found".into()))?;
        self.repo.soft_delete(company_id, id).await
    }

    pub async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<AddonGroup>, CoreError> {
        self.repo.find_unsynced(company_id).await
    }

    pub async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        self.repo.mark_synced(company_id, id, updated_at).await
    }

    pub async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
    ) -> Result<Vec<AddonGroup>, CoreError> {
        self.repo.find_updated_since(company_id, since).await
    }

    /// Upsert de sincronização (§7.7 — last-write-wins).
    pub async fn sync_upsert(
        &self,
        company_id: Uuid,
        mut group: AddonGroup,
    ) -> Result<(), CoreError> {
        if group.base.company_id != company_id {
            return Err(CoreError::Validation("Company mismatch".into()));
        }
        group.base.synced = true;
        self.repo.sync_upsert(&group).await
    }

    /// Validação centralizada dos campos do grupo.
    ///
    /// Regras aplicadas (AI_RULES.md §11):
    /// - `name` não-vazio.
    /// - `selection` ∈ {"single","multi"}.
    /// - `min_select` ≥ 0; `max_select` ≥ 0; quando `max_select > 0`,
    ///   precisa ser ≥ `min_select`.
    /// - `selection = "single"`: `min_select` ∈ {0,1} e `max_select`
    ///   é tratado como 1 (forçamos via clamp aqui para evitar UI
    ///   inconsistente).
    ///
    /// Ajusta `min`/`max` antes de validar, evitando rejeitar grupos
    /// criados em versões anteriores do schema (que aceitavam
    /// `single + max=0`) e payloads de UI que esqueçam de setar
    /// `max=1` em "single". Mantém o invariante "single ⇒ max=1"
    /// sem prender o operador no modal de edição.
    fn normalize(selection: &str, min: i32, max: i32) -> (i32, i32) {
        if selection == "single" {
            let normalized_min = if min > 1 { 1 } else { min.max(0) };
            return (normalized_min, 1);
        }
        (min.max(0), max.max(0))
    }

    fn validate(name: &str, selection: &str, min: i32, max: i32) -> Result<(), CoreError> {
        if name.trim().is_empty() {
            return Err(CoreError::Validation("AddonGroup name is required".into()));
        }
        if !matches!(selection, "single" | "multi") {
            return Err(CoreError::Validation(format!(
                "Unknown selection: '{selection}' (expected single|multi)"
            )));
        }
        if min < 0 || max < 0 {
            return Err(CoreError::Validation("min/max cannot be negative".into()));
        }
        if max > 0 && max < min {
            return Err(CoreError::Validation("max_select must be >= min_select".into()));
        }
        if selection == "single" {
            // Em "single" o teto é sempre 1; aceitar valores diferentes
            // tornaria a UI inconsistente (overlay já trata como rádio).
            if min > 1 {
                return Err(CoreError::Validation(
                    "single selection: min_select must be 0 or 1".into(),
                ));
            }
            if max != 1 {
                return Err(CoreError::Validation(
                    "single selection: max_select must be 1".into(),
                ));
            }
        }
        Ok(())
    }
}
