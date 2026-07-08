use std::sync::Arc;

use std::sync::Mutex;

use async_trait::async_trait;
use chrono::NaiveDateTime;
use uuid::Uuid;
use rust_decimal_macros::dec;

use letaf_core::error::CoreError;
use letaf_core::product::model::Product;
use letaf_core::product::repository::{ProductRepository, StockAdjustResult};
use letaf_core::product::service::ProductService;
use letaf_core::product::stock_movement::StockMovement;

/// Mock in-memory do ProductRepository para testes unitários.
///
/// Regras aplicadas (AI_RULES.md §10):
/// - Acesso a dados via trait (abstração)
/// - Permite testar service sem banco real
struct MockProductRepo {
    items: Mutex<Vec<Product>>,
}

impl MockProductRepo {
    fn new() -> Self {
        Self { items: Mutex::new(Vec::new()) }
    }
}

#[async_trait]
impl ProductRepository for MockProductRepo {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Product>, CoreError> {
        let items = self.items.lock().unwrap();
        Ok(items.iter().find(|p| p.base.id == id && p.base.company_id == company_id && p.base.deleted_at.is_none()).cloned())
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<Product>, CoreError> {
        let items = self.items.lock().unwrap();
        Ok(items.iter().filter(|p| p.base.company_id == company_id && p.base.deleted_at.is_none()).cloned().collect())
    }

    async fn find_by_ids(&self, company_id: Uuid, ids: &[Uuid]) -> Result<Vec<Product>, CoreError> {
        let items = self.items.lock().unwrap();
        Ok(items.iter()
            .filter(|p| p.base.company_id == company_id && p.base.deleted_at.is_none() && ids.contains(&p.base.id))
            .cloned()
            .collect())
    }

    async fn create(&self, product: &Product) -> Result<(), CoreError> {
        self.items.lock().unwrap().push(product.clone());
        Ok(())
    }

    async fn update(&self, product: &Product) -> Result<(), CoreError> {
        let mut items = self.items.lock().unwrap();
        if let Some(existing) = items.iter_mut().find(|p| p.base.id == product.base.id) {
            *existing = product.clone();
        }
        Ok(())
    }

    async fn update_atomic(
        &self,
        product: &Product,
        stock_delta: f64,
        group_ids: &[Uuid],
    ) -> Result<(), CoreError> {
        let mut items = self.items.lock().unwrap();
        if let Some(existing) = items.iter_mut().find(|p| p.base.id == product.base.id) {
            *existing = product.clone();
            if stock_delta.abs() > f64::EPSILON && !product.unlimited_stock {
                let new_qty = existing.stock_quantity + stock_delta;
                if new_qty < 0.0 {
                    return Err(CoreError::Validation(
                        "Estoque insuficiente para o ajuste".into(),
                    ));
                }
                existing.stock_quantity = new_qty;
            }
            existing.addon_group_ids = group_ids.to_vec();
        }
        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let mut items = self.items.lock().unwrap();
        if let Some(p) = items.iter_mut().find(|p| p.base.id == id && p.base.company_id == company_id) {
            p.base.deleted_at = Some(chrono::Utc::now().naive_utc());
        }
        Ok(())
    }

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Product>, CoreError> {
        let items = self.items.lock().unwrap();
        Ok(items.iter().filter(|p| p.base.company_id == company_id && !p.base.synced).cloned().collect())
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid, _updated_at: NaiveDateTime) -> Result<(), CoreError> {
        let mut items = self.items.lock().unwrap();
        if let Some(p) = items.iter_mut().find(|p| p.base.id == id && p.base.company_id == company_id) {
            p.base.synced = true;
        }
        Ok(())
    }

    async fn sync_upsert(&self, product: &Product) -> Result<(), CoreError> {
        let mut items = self.items.lock().unwrap();
        if let Some(existing) = items.iter_mut().find(|p| p.base.id == product.base.id) {
            *existing = product.clone();
        } else {
            items.push(product.clone());
        }
        Ok(())
    }

    async fn find_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<Product>, CoreError> {
        let items = self.items.lock().unwrap();
        Ok(items.iter().filter(|p| p.base.company_id == company_id && p.base.updated_at > since).cloned().collect())
    }

    async fn find_active(&self, company_id: Uuid) -> Result<Vec<Product>, CoreError> {
        let items = self.items.lock().unwrap();
        Ok(items.iter().filter(|p| p.base.company_id == company_id && p.base.deleted_at.is_none() && p.active).cloned().collect())
    }

    async fn toggle_active(&self, company_id: Uuid, id: Uuid, active: bool) -> Result<(), CoreError> {
        let mut items = self.items.lock().unwrap();
        if let Some(p) = items.iter_mut().find(|p| p.base.id == id && p.base.company_id == company_id) {
            p.active = active;
        }
        Ok(())
    }

    async fn toggle_web_visible(&self, company_id: Uuid, id: Uuid, visible: bool) -> Result<(), CoreError> {
        let mut items = self.items.lock().unwrap();
        if let Some(p) = items.iter_mut().find(|p| p.base.id == id && p.base.company_id == company_id) {
            p.web_visible = visible;
        }
        Ok(())
    }

    async fn find_addon_group_ids(&self, company_id: Uuid, product_id: Uuid) -> Result<Vec<Uuid>, CoreError> {
        let items = self.items.lock().unwrap();
        Ok(items.iter()
            .find(|p| p.base.id == product_id && p.base.company_id == company_id)
            .map(|p| p.addon_group_ids.clone())
            .unwrap_or_default())
    }

    async fn try_adjust_stock(&self, company_id: Uuid, product_id: Uuid, delta: f64) -> Result<StockAdjustResult, CoreError> {
        let mut items = self.items.lock().unwrap();
        let Some(p) = items.iter_mut()
            .find(|p| p.base.id == product_id && p.base.company_id == company_id && p.base.deleted_at.is_none())
        else {
            return Ok(StockAdjustResult::NotFound);
        };
        if p.unlimited_stock {
            return Ok(StockAdjustResult::Unlimited);
        }
        let new_qty = p.stock_quantity + delta;
        if new_qty < 0.0 {
            return Ok(StockAdjustResult::Insufficient);
        }
        p.stock_quantity = new_qty;
        p.base.updated_at = chrono::Utc::now().naive_utc();
        p.base.synced = false;
        Ok(StockAdjustResult::Adjusted)
    }

    async fn replace_addon_groups(&self, company_id: Uuid, product_id: Uuid, group_ids: &[Uuid]) -> Result<(), CoreError> {
        let mut items = self.items.lock().unwrap();
        if let Some(p) = items.iter_mut().find(|p| p.base.id == product_id && p.base.company_id == company_id) {
            p.addon_group_ids = group_ids.to_vec();
        }
        Ok(())
    }

    // Ledger de estoque — stubs suficientes para os testes do service.
    async fn find_unsynced_stock_movements(&self, _company_id: Uuid) -> Result<Vec<StockMovement>, CoreError> {
        Ok(Vec::new())
    }
    async fn mark_stock_movement_synced(&self, _company_id: Uuid, _id: Uuid, _updated_at: NaiveDateTime) -> Result<(), CoreError> {
        Ok(())
    }
    async fn apply_stock_movement(&self, movement: &StockMovement) -> Result<(), CoreError> {
        // Aplica o delta ao produto (comportamento mínimo p/ eventuais testes).
        let mut items = self.items.lock().unwrap();
        if let Some(p) = items.iter_mut().find(|p| p.base.id == movement.product_id && !p.unlimited_stock) {
            p.stock_quantity += movement.delta;
        }
        Ok(())
    }
    async fn find_stock_movements_updated_since(&self, _company_id: Uuid, _since: NaiveDateTime) -> Result<Vec<StockMovement>, CoreError> {
        Ok(Vec::new())
    }
}

fn make_service() -> (ProductService, Uuid) {
    let repo = Arc::new(MockProductRepo::new());
    let cid = Uuid::new_v4();
    (ProductService::new(repo), cid)
}

// ── Testes de criação ──────────────────────────────────

#[tokio::test]
async fn create_product_ok() {
    let (svc, cid) = make_service();
    let result = svc.create(cid, "Notebook".into(), Some("Desc".into()), None, None, Some(dec!(2999.90)), None, 10.0, 0.0, false, Some("NB-001".into()), "un".into(), letaf_core::product::model::BalanceMode::Weight, None, None, None, None, None, None, None, Vec::new(), None).await;
    assert!(result.is_ok());
    let p = result.unwrap();
    assert_eq!(p.name, "Notebook");
    assert_eq!(p.description.as_deref(), Some("Desc"));
    assert_eq!(p.price, Some(dec!(2999.90)));
    assert_eq!(p.stock_quantity, 10.0);
    assert_eq!(p.barcode.as_deref(), Some("NB-001"));
    assert_eq!(p.unit, "un");
    assert_eq!(p.base.company_id, cid);
    assert!(!p.base.synced);
}

#[tokio::test]
async fn create_product_empty_name_fails() {
    let (svc, cid) = make_service();
    let result = svc.create(cid, "".into(), None, None, None, Some(dec!(10.0)), None, 0.0, 0.0, false, None, "un".into(), letaf_core::product::model::BalanceMode::Weight, None, None, None, None, None, None, None, Vec::new(), None).await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), CoreError::Validation("Product name is required".into()));
}

#[tokio::test]
async fn create_product_negative_price_fails() {
    let (svc, cid) = make_service();
    let result = svc.create(cid, "X".into(), None, None, None, Some(dec!(-1.0)), None, 0.0, 0.0, false, None, "un".into(), letaf_core::product::model::BalanceMode::Weight, None, None, None, None, None, None, None, Vec::new(), None).await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), CoreError::Validation("Price cannot be negative".into()));
}

#[tokio::test]
async fn create_product_negative_stock_fails() {
    let (svc, cid) = make_service();
    let result = svc.create(cid, "X".into(), None, None, None, Some(dec!(10.0)), None, -5.0, 0.0, false, None, "un".into(), letaf_core::product::model::BalanceMode::Weight, None, None, None, None, None, None, None, Vec::new(), None).await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), CoreError::Validation("Stock quantity cannot be negative".into()));
}

// ── Testes de update ───────────────────────────────────

#[tokio::test]
async fn update_product_ok() {
    let (svc, cid) = make_service();
    let created = svc.create(cid, "Old".into(), None, None, None, Some(dec!(10.0)), None, 1.0, 0.0, false, None, "un".into(), letaf_core::product::model::BalanceMode::Weight, None, None, None, None, None, None, None, Vec::new(), None).await.unwrap();

    let updated = svc.update(cid, created.base.id, "New".into(), Some("Updated desc".into()), None, None, Some(dec!(20.0)), None, 5.0, 0.0, false, Some("SKU-1".into()), "kg".into(), letaf_core::product::model::BalanceMode::Weight, None, None, None, None, None, None, None, Vec::new(), None).await.unwrap();

    assert_eq!(updated.name, "New");
    assert_eq!(updated.description.as_deref(), Some("Updated desc"));
    assert_eq!(updated.price, Some(dec!(20.0)));
    assert_eq!(updated.stock_quantity, 5.0);
    assert_eq!(updated.barcode.as_deref(), Some("SKU-1"));
    assert_eq!(updated.unit, "kg");
    assert!(!updated.base.synced);
    assert!(updated.base.updated_at > created.base.updated_at);
}

#[tokio::test]
async fn update_product_not_found_fails() {
    let (svc, cid) = make_service();
    let result = svc.update(cid, Uuid::new_v4(), "X".into(), None, None, None, Some(dec!(1.0)), None, 0.0, 0.0, false, None, "un".into(), letaf_core::product::model::BalanceMode::Weight, None, None, None, None, None, None, None, Vec::new(), None).await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), CoreError::NotFound("Product not found".into()));
}

#[tokio::test]
async fn update_product_empty_name_fails() {
    let (svc, cid) = make_service();
    let created = svc.create(cid, "Test".into(), None, None, None, Some(dec!(10.0)), None, 1.0, 0.0, false, None, "un".into(), letaf_core::product::model::BalanceMode::Weight, None, None, None, None, None, None, None, Vec::new(), None).await.unwrap();
    let result = svc.update(cid, created.base.id, "".into(), None, None, None, Some(dec!(10.0)), None, 1.0, 0.0, false, None, "un".into(), letaf_core::product::model::BalanceMode::Weight, None, None, None, None, None, None, None, Vec::new(), None).await;
    assert_eq!(result.unwrap_err(), CoreError::Validation("Product name is required".into()));
}

// ── Testes de soft delete ──────────────────────────────

#[tokio::test]
async fn soft_delete_product_ok() {
    let (svc, cid) = make_service();
    let created = svc.create(cid, "ToDelete".into(), None, None, None, None, None, 0.0, 0.0, false, None, "un".into(), letaf_core::product::model::BalanceMode::Weight, None, None, None, None, None, None, None, Vec::new(), None).await.unwrap();
    assert!(svc.soft_delete(cid, created.base.id).await.is_ok());
    let all = svc.find_all(cid).await.unwrap();
    assert!(all.is_empty());
}

#[tokio::test]
async fn soft_delete_product_not_found_fails() {
    let (svc, cid) = make_service();
    let result = svc.soft_delete(cid, Uuid::new_v4()).await;
    assert_eq!(result.unwrap_err(), CoreError::NotFound("Product not found".into()));
}

// ── Testes de isolamento multi-tenant (§11) ────────────

#[tokio::test]
async fn product_isolation_between_companies() {
    let (svc, cid1) = make_service();
    let cid2 = Uuid::new_v4();

    svc.create(cid1, "Company1 Product".into(), None, None, None, Some(dec!(10.0)), None, 1.0, 0.0, false, None, "un".into(), letaf_core::product::model::BalanceMode::Weight, None, None, None, None, None, None, None, Vec::new(), None).await.unwrap();
    svc.create(cid2, "Company2 Product".into(), None, None, None, Some(dec!(20.0)), None, 2.0, 0.0, false, None, "un".into(), letaf_core::product::model::BalanceMode::Weight, None, None, None, None, None, None, None, Vec::new(), None).await.unwrap();

    let items_c1 = svc.find_all(cid1).await.unwrap();
    let items_c2 = svc.find_all(cid2).await.unwrap();

    assert_eq!(items_c1.len(), 1);
    assert_eq!(items_c1[0].name, "Company1 Product");
    assert_eq!(items_c2.len(), 1);
    assert_eq!(items_c2[0].name, "Company2 Product");
}

// ── Testes de sync (§7) ───────────────────────────────

#[tokio::test]
async fn product_created_as_unsynced() {
    let (svc, cid) = make_service();
    let p = svc.create(cid, "X".into(), None, None, None, None, None, 0.0, 0.0, false, None, "un".into(), letaf_core::product::model::BalanceMode::Weight, None, None, None, None, None, None, None, Vec::new(), None).await.unwrap();
    assert!(!p.base.synced);
    let unsynced = svc.find_unsynced(cid).await.unwrap();
    assert_eq!(unsynced.len(), 1);
}

#[tokio::test]
async fn mark_synced_works() {
    let (svc, cid) = make_service();
    let p = svc.create(cid, "X".into(), None, None, None, None, None, 0.0, 0.0, false, None, "un".into(), letaf_core::product::model::BalanceMode::Weight, None, None, None, None, None, None, None, Vec::new(), None).await.unwrap();
    svc.mark_synced(cid, p.base.id, p.base.updated_at).await.unwrap();
    let unsynced = svc.find_unsynced(cid).await.unwrap();
    assert!(unsynced.is_empty());
}

#[tokio::test]
async fn sync_upsert_validates_company() {
    let (svc, cid) = make_service();
    let wrong_cid = Uuid::new_v4();
    let p = Product::new(cid, "X".into(), None, None, None, None, None, 0.0, 0.0, false, None, "un".into(), letaf_core::product::model::BalanceMode::Weight, None, None, None, None, None, None, None);
    let result = svc.sync_upsert(wrong_cid, p).await;
    assert_eq!(result.unwrap_err(), CoreError::Validation("Company mismatch".into()));
}
