use std::sync::Arc;

use uuid::Uuid;

use super::model::{Banner, ITEM_TYPES};
use super::repository::BannerRepository;
use crate::error::CoreError;

/// Service para o domínio Banner.
///
/// Regras aplicadas (AI_RULES.md §1, §9, §11):
/// - Orquestra regras de negócio (validações + repo).
/// - Não confia no frontend: `item_type` é validado contra a
///   allowlist do core; `item_id`/`item_url` exigidos conforme o tipo.
/// - Limite de 20MB para a imagem (mesmo limite do exemplo
///   teste.naturalle.app e dos produtos). Aproximação: o base64 é
///   ~1.37x o tamanho original, então 20MB binário ≈ 27.4MB base64.
pub struct BannerService {
    repo: Arc<dyn BannerRepository>,
}

const IMAGE_MAX_BASE64_BYTES: usize = 28 * 1024 * 1024;

impl BannerService {
    pub fn new(repo: Arc<dyn BannerRepository>) -> Self {
        Self { repo }
    }

    pub async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Banner>, CoreError> {
        self.repo.find_by_id(company_id, id).await
    }

    pub async fn find_all(&self, company_id: Uuid) -> Result<Vec<Banner>, CoreError> {
        self.repo.find_all(company_id).await
    }

    /// Banners ativos (rota pública).
    pub async fn find_active(&self, company_id: Uuid) -> Result<Vec<Banner>, CoreError> {
        self.repo.find_active(company_id).await
    }

    pub async fn create(
        &self,
        company_id: Uuid,
        title: String,
        image_data: String,
        item_type: String,
        item_id: Option<Uuid>,
        item_url: Option<String>,
    ) -> Result<Banner, CoreError> {
        validate(&title, &image_data, &item_type, item_id, item_url.as_deref())?;
        let banner = Banner::new(company_id, title, image_data, item_type, item_id, item_url);
        self.repo.create(&banner).await?;
        Ok(banner)
    }

    pub async fn update(
        &self,
        company_id: Uuid,
        id: Uuid,
        title: String,
        image_data: String,
        item_type: String,
        item_id: Option<Uuid>,
        item_url: Option<String>,
    ) -> Result<Banner, CoreError> {
        validate(&title, &image_data, &item_type, item_id, item_url.as_deref())?;
        let mut banner = self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("Banner not found".into()))?;
        banner.title = title;
        banner.image_data = image_data;
        banner.item_type = item_type;
        banner.item_id = item_id;
        banner.item_url = item_url;
        banner.base.updated_at = chrono::Utc::now().naive_utc();
        banner.base.synced = false;
        self.repo.update(&banner).await?;
        Ok(banner)
    }

    pub async fn set_active(
        &self,
        company_id: Uuid,
        id: Uuid,
        active: bool,
    ) -> Result<(), CoreError> {
        self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("Banner not found".into()))?;
        self.repo.set_active(company_id, id, active).await
    }

    pub async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("Banner not found".into()))?;
        self.repo.soft_delete(company_id, id).await
    }

    pub async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Banner>, CoreError> {
        self.repo.find_unsynced(company_id).await
    }

    pub async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        self.repo.mark_synced(company_id, id, updated_at).await
    }

    pub async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
    ) -> Result<Vec<Banner>, CoreError> {
        self.repo.find_updated_since(company_id, since).await
    }

    pub async fn sync_upsert(
        &self,
        company_id: Uuid,
        mut banner: Banner,
    ) -> Result<(), CoreError> {
        if banner.base.company_id != company_id {
            return Err(CoreError::Validation("Company mismatch".into()));
        }
        banner.base.synced = true;
        self.repo.sync_upsert(&banner).await
    }
}

/// Validação central usada tanto em create quanto em update.
fn validate(
    title: &str,
    image_data: &str,
    item_type: &str,
    item_id: Option<Uuid>,
    item_url: Option<&str>,
) -> Result<(), CoreError> {
    if title.trim().is_empty() {
        return Err(CoreError::Validation("Banner title is required".into()));
    }
    if image_data.trim().is_empty() {
        return Err(CoreError::Validation("Banner image is required".into()));
    }
    if image_data.len() > IMAGE_MAX_BASE64_BYTES {
        return Err(CoreError::Validation(
            "Banner image exceeds 20MB limit".into(),
        ));
    }
    if !ITEM_TYPES.contains(&item_type) {
        return Err(CoreError::Validation(format!(
            "Unknown banner item type '{item_type}'"
        )));
    }
    match item_type {
        "product" => {
            if item_id.is_none() {
                return Err(CoreError::Validation(
                    "Banner with type 'product' requires item_id".into(),
                ));
            }
        }
        "url" => {
            let url = item_url.map(str::trim).unwrap_or("");
            if url.is_empty() {
                return Err(CoreError::Validation(
                    "Banner with type 'url' requires item_url".into(),
                ));
            }
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                return Err(CoreError::Validation(
                    "Banner URL must start with http:// or https://".into(),
                ));
            }
        }
        _ => {}
    }
    Ok(())
}
