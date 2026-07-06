//! Testes do `AddonGroupService` — foco nas validações de
//! `selection` × `min_select` × `max_select` (Fase 4).

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::NaiveDateTime;
use uuid::Uuid;

use letaf_core::addon_group::model::AddonGroup;
use letaf_core::addon_group::repository::AddonGroupRepository;
use letaf_core::addon_group::service::AddonGroupService;
use letaf_core::error::CoreError;

struct MockRepo {
    items: Mutex<Vec<AddonGroup>>,
}

impl MockRepo {
    fn new() -> Self { Self { items: Mutex::new(Vec::new()) } }
}

#[async_trait]
impl AddonGroupRepository for MockRepo {
    async fn find_by_id(&self, cid: Uuid, id: Uuid) -> Result<Option<AddonGroup>, CoreError> {
        Ok(self.items.lock().unwrap().iter()
            .find(|g| g.base.id == id && g.base.company_id == cid && g.base.deleted_at.is_none())
            .cloned())
    }
    async fn find_all(&self, cid: Uuid) -> Result<Vec<AddonGroup>, CoreError> {
        Ok(self.items.lock().unwrap().iter()
            .filter(|g| g.base.company_id == cid && g.base.deleted_at.is_none())
            .cloned().collect())
    }
    async fn find_by_product(&self, _cid: Uuid, _pid: Uuid) -> Result<Vec<AddonGroup>, CoreError> {
        Ok(Vec::new())
    }
    async fn create(&self, g: &AddonGroup) -> Result<(), CoreError> {
        self.items.lock().unwrap().push(g.clone()); Ok(())
    }
    async fn update(&self, g: &AddonGroup) -> Result<(), CoreError> {
        let mut items = self.items.lock().unwrap();
        if let Some(existing) = items.iter_mut().find(|i| i.base.id == g.base.id) {
            *existing = g.clone();
        }
        Ok(())
    }
    async fn soft_delete(&self, cid: Uuid, id: Uuid) -> Result<(), CoreError> {
        let mut items = self.items.lock().unwrap();
        if let Some(g) = items.iter_mut().find(|i| i.base.id == id && i.base.company_id == cid) {
            g.base.deleted_at = Some(chrono::Utc::now().naive_utc());
        }
        Ok(())
    }
    async fn find_unsynced(&self, _cid: Uuid) -> Result<Vec<AddonGroup>, CoreError> { Ok(Vec::new()) }
    async fn mark_synced(&self, _cid: Uuid, _id: Uuid, _updated_at: NaiveDateTime) -> Result<(), CoreError> { Ok(()) }
    async fn sync_upsert(&self, _g: &AddonGroup) -> Result<(), CoreError> { Ok(()) }
    async fn find_updated_since(&self, _cid: Uuid, _since: NaiveDateTime) -> Result<Vec<AddonGroup>, CoreError> { Ok(Vec::new()) }
}

fn make_service() -> (AddonGroupService, Uuid) {
    let repo = Arc::new(MockRepo::new());
    (AddonGroupService::new(repo), Uuid::new_v4())
}

#[tokio::test]
async fn create_single_with_max_1_succeeds() {
    let (svc, cid) = make_service();
    let r = svc.create(cid, "Borda".into(), "single".into(), 0, 1).await;
    assert!(r.is_ok());
}

#[tokio::test]
async fn create_single_normalizes_max_0_to_1() {
    // Single nunca pode ter max=0 (radio sem teto faz sentido?), mas
    // grupos legados podem ter sido criados assim. A normalização
    // força max=1 sem rejeitar o input — operador não fica preso.
    let (svc, cid) = make_service();
    let g = svc.create(cid, "Borda".into(), "single".into(), 0, 0).await.unwrap();
    assert_eq!(g.max_select, 1);
    assert_eq!(g.min_select, 0);
}

#[tokio::test]
async fn create_single_normalizes_max_2_to_1() {
    let (svc, cid) = make_service();
    let g = svc.create(cid, "Borda".into(), "single".into(), 0, 2).await.unwrap();
    assert_eq!(g.max_select, 1);
}

#[tokio::test]
async fn create_single_normalizes_min_above_1() {
    // Em "single" o min sempre vira no máximo 1 (não faz sentido
    // exigir >1 escolhas num radio).
    let (svc, cid) = make_service();
    let g = svc.create(cid, "Borda".into(), "single".into(), 2, 1).await.unwrap();
    assert_eq!(g.min_select, 1);
    assert_eq!(g.max_select, 1);
}

#[tokio::test]
async fn create_multi_with_max_0_succeeds() {
    let (svc, cid) = make_service();
    let r = svc.create(cid, "Toppings".into(), "multi".into(), 0, 0).await;
    assert!(r.is_ok());
}

#[tokio::test]
async fn create_multi_with_min_above_max_fails() {
    let (svc, cid) = make_service();
    let r = svc.create(cid, "T".into(), "multi".into(), 5, 3).await;
    assert!(r.is_err());
}

#[tokio::test]
async fn unknown_selection_fails() {
    let (svc, cid) = make_service();
    let r = svc.create(cid, "X".into(), "any".into(), 0, 1).await;
    assert!(r.is_err());
}

#[tokio::test]
async fn empty_name_fails() {
    let (svc, cid) = make_service();
    let r = svc.create(cid, "  ".into(), "single".into(), 0, 1).await;
    assert!(r.is_err());
}

#[tokio::test]
async fn negative_min_or_max_clamped_to_zero() {
    // Valores negativos do payload são normalizados para 0 antes da
    // validação (defesa em profundidade contra UI bugada). O service
    // não rejeita o pedido — só "limpa" o input.
    let (svc, cid) = make_service();
    let g1 = svc.create(cid, "X".into(), "multi".into(), -1, 0).await.unwrap();
    assert_eq!(g1.min_select, 0);
    let g2 = svc.create(cid, "Y".into(), "multi".into(), 0, -1).await.unwrap();
    assert_eq!(g2.max_select, 0);
}
