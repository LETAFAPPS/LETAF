use std::sync::Arc;

use std::sync::Mutex;

use async_trait::async_trait;
use chrono::NaiveDateTime;
use uuid::Uuid;

use letaf_core::category::model::Category;
use letaf_core::category::repository::CategoryRepository;
use letaf_core::category::service::CategoryService;
use letaf_core::error::CoreError;

/// Mock in-memory do CategoryRepository.
struct MockCategoryRepo {
    items: Mutex<Vec<Category>>,
}

impl MockCategoryRepo {
    fn new() -> Self {
        Self { items: Mutex::new(Vec::new()) }
    }
}

#[async_trait]
impl CategoryRepository for MockCategoryRepo {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Category>, CoreError> {
        let items = self.items.lock().unwrap();
        Ok(items.iter().find(|c| c.base.id == id && c.base.company_id == company_id && c.base.deleted_at.is_none()).cloned())
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<Category>, CoreError> {
        let items = self.items.lock().unwrap();
        Ok(items.iter().filter(|c| c.base.company_id == company_id && c.base.deleted_at.is_none()).cloned().collect())
    }

    async fn create(&self, category: &Category) -> Result<(), CoreError> {
        self.items.lock().unwrap().push(category.clone());
        Ok(())
    }

    async fn update(&self, category: &Category) -> Result<(), CoreError> {
        let mut items = self.items.lock().unwrap();
        if let Some(existing) = items.iter_mut().find(|c| c.base.id == category.base.id) {
            *existing = category.clone();
        }
        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let mut items = self.items.lock().unwrap();
        if let Some(c) = items.iter_mut().find(|c| c.base.id == id && c.base.company_id == company_id) {
            c.base.deleted_at = Some(chrono::Utc::now().naive_utc());
        }
        Ok(())
    }

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Category>, CoreError> {
        let items = self.items.lock().unwrap();
        Ok(items.iter().filter(|c| c.base.company_id == company_id && !c.base.synced).cloned().collect())
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let mut items = self.items.lock().unwrap();
        if let Some(c) = items.iter_mut().find(|c| c.base.id == id && c.base.company_id == company_id) {
            c.base.synced = true;
        }
        Ok(())
    }

    async fn sync_upsert(&self, category: &Category) -> Result<(), CoreError> {
        let mut items = self.items.lock().unwrap();
        if let Some(existing) = items.iter_mut().find(|c| c.base.id == category.base.id) {
            *existing = category.clone();
        } else {
            items.push(category.clone());
        }
        Ok(())
    }

    async fn find_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<Category>, CoreError> {
        let items = self.items.lock().unwrap();
        Ok(items.iter().filter(|c| c.base.company_id == company_id && c.base.updated_at > since).cloned().collect())
    }
}

fn make_service() -> (CategoryService, Uuid) {
    let repo = Arc::new(MockCategoryRepo::new());
    let cid = Uuid::new_v4();
    (CategoryService::new(repo), cid)
}

// ── Criação ────────────────────────────────────────────

#[tokio::test]
async fn create_category_ok() {
    let (svc, cid) = make_service();
    let result = svc.create(cid, "Eletrônicos".into(), Some("Desc".into()), None).await;
    assert!(result.is_ok());
    let c = result.unwrap();
    assert_eq!(c.name, "Eletrônicos");
    assert_eq!(c.description.as_deref(), Some("Desc"));
    assert!(!c.base.synced);
}

#[tokio::test]
async fn create_category_empty_name_fails() {
    let (svc, cid) = make_service();
    let result = svc.create(cid, "".into(), None, None).await;
    assert_eq!(result.unwrap_err(), CoreError::Validation("Category name is required".into()));
}

#[tokio::test]
async fn create_category_whitespace_name_fails() {
    let (svc, cid) = make_service();
    let result = svc.create(cid, "   ".into(), None, None).await;
    assert_eq!(result.unwrap_err(), CoreError::Validation("Category name is required".into()));
}

// ── Update ─────────────────────────────────────────────

#[tokio::test]
async fn update_category_ok() {
    let (svc, cid) = make_service();
    let created = svc.create(cid, "Old".into(), None, None).await.unwrap();
    let updated = svc.update(cid, created.base.id, "New".into(), Some("Updated".into()), None).await.unwrap();

    assert_eq!(updated.name, "New");
    assert_eq!(updated.description.as_deref(), Some("Updated"));
    assert!(!updated.base.synced);
    assert!(updated.base.updated_at > created.base.updated_at);
}

#[tokio::test]
async fn update_category_not_found_fails() {
    let (svc, cid) = make_service();
    let result = svc.update(cid, Uuid::new_v4(), "X".into(), None, None).await;
    assert_eq!(result.unwrap_err(), CoreError::NotFound("Category not found".into()));
}

#[tokio::test]
async fn update_category_empty_name_fails() {
    let (svc, cid) = make_service();
    let created = svc.create(cid, "Test".into(), None, None).await.unwrap();
    let result = svc.update(cid, created.base.id, "".into(), None, None).await;
    assert_eq!(result.unwrap_err(), CoreError::Validation("Category name is required".into()));
}

// ── Soft delete ────────────────────────────────────────

#[tokio::test]
async fn soft_delete_category_ok() {
    let (svc, cid) = make_service();
    let created = svc.create(cid, "ToDelete".into(), None, None).await.unwrap();
    svc.soft_delete(cid, created.base.id).await.unwrap();
    assert!(svc.find_all(cid).await.unwrap().is_empty());
}

#[tokio::test]
async fn soft_delete_category_not_found_fails() {
    let (svc, cid) = make_service();
    let result = svc.soft_delete(cid, Uuid::new_v4()).await;
    assert_eq!(result.unwrap_err(), CoreError::NotFound("Category not found".into()));
}

// ── Isolamento multi-tenant (§11) ─────────────────────

#[tokio::test]
async fn category_isolation_between_companies() {
    let (svc, cid1) = make_service();
    let cid2 = Uuid::new_v4();

    svc.create(cid1, "Cat A".into(), None, None).await.unwrap();
    svc.create(cid2, "Cat B".into(), None, None).await.unwrap();

    assert_eq!(svc.find_all(cid1).await.unwrap().len(), 1);
    assert_eq!(svc.find_all(cid2).await.unwrap().len(), 1);
    assert_eq!(svc.find_all(cid1).await.unwrap()[0].name, "Cat A");
}

// ── Sync (§7) ──────────────────────────────────────────

#[tokio::test]
async fn category_created_as_unsynced() {
    let (svc, cid) = make_service();
    svc.create(cid, "X".into(), None, None).await.unwrap();
    assert_eq!(svc.find_unsynced(cid).await.unwrap().len(), 1);
}

#[tokio::test]
async fn sync_upsert_validates_company() {
    let (svc, cid) = make_service();
    let wrong_cid = Uuid::new_v4();
    let c = Category::new(cid, "X".into(), None);
    let result = svc.sync_upsert(wrong_cid, c).await;
    assert_eq!(result.unwrap_err(), CoreError::Validation("Company mismatch".into()));
}

#[tokio::test]
async fn create_accepts_valid_icon_slug() {
    let (svc, cid) = make_service();
    let cat = svc.create(cid, "Sorvetes".into(), None, Some("ice-cream".into())).await.unwrap();
    assert_eq!(cat.icon_name.as_deref(), Some("ice-cream"));
}

#[tokio::test]
async fn create_rejects_unknown_icon_slug() {
    let (svc, cid) = make_service();
    let result = svc.create(cid, "Foo".into(), None, Some("not-a-real-icon".into())).await;
    assert!(matches!(result, Err(CoreError::Validation(_))));
}

#[tokio::test]
async fn create_normalizes_blank_icon_to_none() {
    // String em branco deve virar None (operador limpou o campo
    // sem selecionar nenhum ícone).
    let (svc, cid) = make_service();
    let cat = svc.create(cid, "Foo".into(), None, Some("   ".into())).await.unwrap();
    assert_eq!(cat.icon_name, None);
}

#[tokio::test]
async fn update_can_change_icon() {
    let (svc, cid) = make_service();
    let cat = svc.create(cid, "Foo".into(), None, Some("drink".into())).await.unwrap();
    let updated = svc.update(cid, cat.base.id, "Foo".into(), None, Some("coffee".into())).await.unwrap();
    assert_eq!(updated.icon_name.as_deref(), Some("coffee"));
}

#[tokio::test]
async fn update_can_clear_icon() {
    let (svc, cid) = make_service();
    let cat = svc.create(cid, "Foo".into(), None, Some("drink".into())).await.unwrap();
    let updated = svc.update(cid, cat.base.id, "Foo".into(), None, None).await.unwrap();
    assert_eq!(updated.icon_name, None);
}
