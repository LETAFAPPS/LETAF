//! Regressão da 5ª auditoria (SEG#1, ALTA): a tela de Colaboradores gerencia
//! APENAS funcionários. `update_employee`/`delete_employee` devem RECUSAR um
//! alvo Admin/SuperAdmin — senão um usuário com `collaborators.edit` (gerente)
//! trocaria a senha do Admin e assumiria a conta (escalada vertical, §11).

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::NaiveDateTime;
use uuid::Uuid;

use letaf_core::auth::model::{User, UserRole};
use letaf_core::auth::repository::UserRepository;
use letaf_core::auth::service::AuthService;
use letaf_core::error::CoreError;

struct MockUserRepo {
    users: Mutex<Vec<User>>,
}

#[async_trait]
impl UserRepository for MockUserRepo {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<User>, CoreError> {
        Ok(self.users.lock().unwrap().iter()
            .find(|u| u.base.id == id && u.base.company_id == company_id)
            .cloned())
    }
    async fn find_by_email(&self, _c: Uuid, _e: &str) -> Result<Option<User>, CoreError> { Ok(None) }
    async fn find_by_email_any(&self, _c: Uuid, _e: &str) -> Result<Option<User>, CoreError> { Ok(None) }
    async fn find_all(&self, company_id: Uuid) -> Result<Vec<User>, CoreError> {
        Ok(self.users.lock().unwrap().iter().filter(|u| u.base.company_id == company_id).cloned().collect())
    }
    async fn create(&self, user: &User) -> Result<(), CoreError> {
        self.users.lock().unwrap().push(user.clone()); Ok(())
    }
    async fn update(&self, user: &User) -> Result<(), CoreError> {
        let mut v = self.users.lock().unwrap();
        if let Some(u) = v.iter_mut().find(|u| u.base.id == user.base.id) { *u = user.clone(); }
        Ok(())
    }
    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        self.users.lock().unwrap().retain(|u| !(u.base.id == id && u.base.company_id == company_id));
        Ok(())
    }
    async fn find_unsynced(&self, _c: Uuid) -> Result<Vec<User>, CoreError> { Ok(vec![]) }
    async fn mark_synced(&self, _c: Uuid, _i: Uuid, _u: NaiveDateTime) -> Result<(), CoreError> { Ok(()) }
    async fn sync_upsert(&self, _u: &User) -> Result<(), CoreError> { Ok(()) }
    async fn find_updated_since(&self, _c: Uuid, _s: NaiveDateTime) -> Result<Vec<User>, CoreError> { Ok(vec![]) }
    async fn find_by_email_global(&self, _e: &str) -> Result<Option<User>, CoreError> { Ok(None) }
}

fn service_with(users: Vec<User>) -> AuthService {
    AuthService::new(Arc::new(MockUserRepo { users: Mutex::new(users) }))
}

#[tokio::test]
async fn delete_employee_recusa_admin() {
    let cid = Uuid::new_v4();
    let admin = User::new(cid, "admin@x.com".into(), "h".into(), "Admin".into(), UserRole::Admin);
    let admin_id = admin.base.id;
    let svc = service_with(vec![admin]);

    let err = svc.delete_employee(cid, admin_id).await;
    assert!(matches!(err, Err(CoreError::Unauthorized(_))),
        "delete_employee deveria recusar alvo Admin, veio: {err:?}");
}

#[tokio::test]
async fn delete_employee_permite_funcionario() {
    let cid = Uuid::new_v4();
    let emp = User::new(cid, "emp@x.com".into(), "h".into(), "Func".into(), UserRole::Employee);
    let emp_id = emp.base.id;
    let svc = service_with(vec![emp]);

    assert!(svc.delete_employee(cid, emp_id).await.is_ok(),
        "delete_employee deveria permitir alvo Employee");
}

#[tokio::test]
async fn update_employee_recusa_admin() {
    let cid = Uuid::new_v4();
    let admin = User::new(cid, "admin@x.com".into(), "h".into(), "Admin".into(), UserRole::Admin);
    let admin_id = admin.base.id;
    let svc = service_with(vec![admin]);

    // Tenta trocar nome/senha do Admin via caminho de funcionário → deve recusar
    // ANTES de qualquer alteração (o guard vem logo após o find_by_id).
    let err = svc.update_employee(cid, admin_id, "Hax".into(), None, Some("nova".into())).await;
    assert!(matches!(err, Err(CoreError::Unauthorized(_))),
        "update_employee deveria recusar alvo Admin, veio: {err:?}");
}
