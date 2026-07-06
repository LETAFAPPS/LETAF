use std::sync::Arc;

use std::sync::Mutex;

use async_trait::async_trait;
use chrono::NaiveDateTime;
use uuid::Uuid;

use letaf_core::customer::model::Customer;
use letaf_core::customer::repository::CustomerRepository;
use letaf_core::customer::service::CustomerService;
use letaf_core::error::CoreError;

/// Mock in-memory do CustomerRepository.
struct MockCustomerRepo {
    items: Mutex<Vec<Customer>>,
}

impl MockCustomerRepo {
    fn new() -> Self {
        Self { items: Mutex::new(Vec::new()) }
    }
}

#[async_trait]
impl CustomerRepository for MockCustomerRepo {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Customer>, CoreError> {
        let items = self.items.lock().unwrap();
        Ok(items.iter().find(|c| c.base.id == id && c.base.company_id == company_id && c.base.deleted_at.is_none()).cloned())
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<Customer>, CoreError> {
        let items = self.items.lock().unwrap();
        Ok(items.iter().filter(|c| c.base.company_id == company_id && c.base.deleted_at.is_none()).cloned().collect())
    }

    async fn find_by_email(&self, company_id: Uuid, email: &str) -> Result<Option<Customer>, CoreError> {
        let items = self.items.lock().unwrap();
        Ok(items.iter().find(|c| c.base.company_id == company_id && c.email.as_deref() == Some(email) && c.base.deleted_at.is_none()).cloned())
    }

    async fn create(&self, customer: &Customer) -> Result<(), CoreError> {
        self.items.lock().unwrap().push(customer.clone());
        Ok(())
    }

    async fn update(&self, customer: &Customer) -> Result<(), CoreError> {
        let mut items = self.items.lock().unwrap();
        if let Some(existing) = items.iter_mut().find(|c| c.base.id == customer.base.id) {
            *existing = customer.clone();
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

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Customer>, CoreError> {
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

    async fn sync_upsert(&self, customer: &Customer) -> Result<(), CoreError> {
        let mut items = self.items.lock().unwrap();
        if let Some(existing) = items.iter_mut().find(|c| c.base.id == customer.base.id) {
            *existing = customer.clone();
        } else {
            items.push(customer.clone());
        }
        Ok(())
    }

    async fn find_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<Customer>, CoreError> {
        let items = self.items.lock().unwrap();
        Ok(items.iter().filter(|c| c.base.company_id == company_id && c.base.updated_at > since).cloned().collect())
    }
}

fn make_service() -> (CustomerService, Uuid) {
    let repo = Arc::new(MockCustomerRepo::new());
    let cid = Uuid::new_v4();
    (CustomerService::new(repo), cid)
}

// ── Criação ────────────────────────────────────────────

#[tokio::test]
async fn create_customer_ok() {
    let (svc, cid) = make_service();
    let result = svc.create(cid, "João".into(), Some("joao@email.com".into()), Some("11999999999".into()), Some("12345678900".into()), None).await;
    assert!(result.is_ok());
    let c = result.unwrap();
    assert_eq!(c.name, "João");
    assert_eq!(c.email.as_deref(), Some("joao@email.com"));
    assert_eq!(c.phone.as_deref(), Some("11999999999"));
    assert_eq!(c.document.as_deref(), Some("12345678900"));
    assert!(!c.base.synced);
}

#[tokio::test]
async fn create_customer_empty_name_fails() {
    let (svc, cid) = make_service();
    let result = svc.create(cid, "".into(), None, None, None, None).await;
    assert_eq!(result.unwrap_err(), CoreError::Validation("Customer name is required".into()));
}

#[tokio::test]
async fn create_customer_whitespace_name_fails() {
    let (svc, cid) = make_service();
    let result = svc.create(cid, "   ".into(), None, None, None, None).await;
    assert_eq!(result.unwrap_err(), CoreError::Validation("Customer name is required".into()));
}

// ── Update ─────────────────────────────────────────────

#[tokio::test]
async fn update_customer_ok() {
    let (svc, cid) = make_service();
    let created = svc.create(cid, "Old".into(), Some("old@x.com".into()), None, None, None).await.unwrap();
    let updated = svc.update(cid, created.base.id, "New".into(), Some("new@x.com".into()), Some("999".into()), Some("DOC".into()), Some("nota".into())).await.unwrap();

    assert_eq!(updated.name, "New");
    assert_eq!(updated.email.as_deref(), Some("new@x.com"));
    assert_eq!(updated.phone.as_deref(), Some("999"));
    assert_eq!(updated.document.as_deref(), Some("DOC"));
    assert!(!updated.base.synced);
    assert!(updated.base.updated_at > created.base.updated_at);
}

#[tokio::test]
async fn update_customer_not_found_fails() {
    let (svc, cid) = make_service();
    let result = svc.update(cid, Uuid::new_v4(), "X".into(), None, None, None, None).await;
    assert_eq!(result.unwrap_err(), CoreError::NotFound("Customer not found".into()));
}

#[tokio::test]
async fn update_customer_empty_name_fails() {
    let (svc, cid) = make_service();
    let created = svc.create(cid, "Test".into(), None, None, None, None).await.unwrap();
    let result = svc.update(cid, created.base.id, "".into(), None, None, None, None).await;
    assert_eq!(result.unwrap_err(), CoreError::Validation("Customer name is required".into()));
}

// ── Soft delete ────────────────────────────────────────

#[tokio::test]
async fn soft_delete_customer_ok() {
    let (svc, cid) = make_service();
    let created = svc.create(cid, "ToDelete".into(), None, None, None, None).await.unwrap();
    svc.soft_delete(cid, created.base.id).await.unwrap();
    assert!(svc.find_all(cid).await.unwrap().is_empty());
}

#[tokio::test]
async fn soft_delete_customer_not_found_fails() {
    let (svc, cid) = make_service();
    let result = svc.soft_delete(cid, Uuid::new_v4()).await;
    assert_eq!(result.unwrap_err(), CoreError::NotFound("Customer not found".into()));
}

// ── Isolamento multi-tenant (§11) ─────────────────────

#[tokio::test]
async fn customer_isolation_between_companies() {
    let (svc, cid1) = make_service();
    let cid2 = Uuid::new_v4();

    svc.create(cid1, "Client A".into(), None, None, None, None).await.unwrap();
    svc.create(cid2, "Client B".into(), None, None, None, None).await.unwrap();

    assert_eq!(svc.find_all(cid1).await.unwrap().len(), 1);
    assert_eq!(svc.find_all(cid2).await.unwrap().len(), 1);
    assert_eq!(svc.find_all(cid1).await.unwrap()[0].name, "Client A");
}

// ── Sync (§7) ──────────────────────────────────────────

#[tokio::test]
async fn customer_created_as_unsynced() {
    let (svc, cid) = make_service();
    svc.create(cid, "X".into(), None, None, None, None).await.unwrap();
    assert_eq!(svc.find_unsynced(cid).await.unwrap().len(), 1);
}

#[tokio::test]
async fn sync_upsert_validates_company() {
    let (svc, cid) = make_service();
    let wrong_cid = Uuid::new_v4();
    let c = Customer::new(cid, "X".into(), None, None, None);
    let result = svc.sync_upsert(wrong_cid, c).await;
    assert_eq!(result.unwrap_err(), CoreError::Validation("Company mismatch".into()));
}

// ── Find by email ──────────────────────────────────────

#[tokio::test]
async fn find_by_email_works() {
    let (svc, cid) = make_service();
    svc.create(cid, "João".into(), Some("joao@x.com".into()), None, None, None).await.unwrap();
    let found = svc.find_by_email(cid, "joao@x.com").await.unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().name, "João");

    let not_found = svc.find_by_email(cid, "nope@x.com").await.unwrap();
    assert!(not_found.is_none());
}
