//! Testes da aritmética de saldo da carteira (`WalletService`) — dinheiro em
//! `Decimal`. Cobrem depósito, saque, cobrança de pedido, estorno e o limite
//! de fiado (floor = -credit_limit). Mock in-memory do repositório (§10).

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::NaiveDateTime;
use rust_decimal_macros::dec;
use uuid::Uuid;

use letaf_core::error::CoreError;
use letaf_core::wallet::model::{WalletAccount, WalletMovement};
use letaf_core::wallet::repository::WalletRepository;
use letaf_core::wallet::service::WalletService;

struct MockWalletRepo {
    accounts: Mutex<Vec<WalletAccount>>,
}
impl MockWalletRepo {
    fn new() -> Self { Self { accounts: Mutex::new(Vec::new()) } }
}

#[async_trait]
impl WalletRepository for MockWalletRepo {
    async fn find_account_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<WalletAccount>, CoreError> {
        Ok(self.accounts.lock().unwrap().iter()
            .find(|a| a.base.id == id && a.base.company_id == company_id).cloned())
    }
    async fn find_account_by_customer(&self, company_id: Uuid, customer_id: Uuid) -> Result<Option<WalletAccount>, CoreError> {
        Ok(self.accounts.lock().unwrap().iter()
            .find(|a| a.customer_id == customer_id && a.base.company_id == company_id).cloned())
    }
    async fn find_all_accounts(&self, _c: Uuid) -> Result<Vec<WalletAccount>, CoreError> { Ok(vec![]) }
    async fn create_account(&self, account: &WalletAccount) -> Result<(), CoreError> {
        self.accounts.lock().unwrap().push(account.clone()); Ok(())
    }
    async fn update_account(&self, account: &WalletAccount) -> Result<(), CoreError> {
        let mut v = self.accounts.lock().unwrap();
        if let Some(a) = v.iter_mut().find(|a| a.base.id == account.base.id) { *a = account.clone(); }
        Ok(())
    }
    async fn apply_movement(&self, account_new_state: &WalletAccount, _m: &WalletMovement, expected_old_balance: rust_decimal::Decimal) -> Result<bool, CoreError> {
        // Concorrência otimista: só aplica se o saldo atual bater com o esperado.
        let mut v = self.accounts.lock().unwrap();
        if let Some(a) = v.iter_mut().find(|a| a.base.id == account_new_state.base.id) {
            if a.balance != expected_old_balance {
                return Ok(false);
            }
            *a = account_new_state.clone();
            return Ok(true);
        }
        Ok(false)
    }
    async fn find_movements_by_account(&self, _c: Uuid, _a: Uuid, _l: i64) -> Result<Vec<WalletMovement>, CoreError> { Ok(vec![]) }
    async fn find_unsynced_accounts(&self, _c: Uuid) -> Result<Vec<WalletAccount>, CoreError> { Ok(vec![]) }
    async fn mark_account_synced(&self, _c: Uuid, _i: Uuid, _u: NaiveDateTime) -> Result<(), CoreError> { Ok(()) }
    async fn find_accounts_updated_since(&self, _c: Uuid, _s: NaiveDateTime) -> Result<Vec<WalletAccount>, CoreError> { Ok(vec![]) }
    async fn sync_upsert_account(&self, _a: &WalletAccount) -> Result<(), CoreError> { Ok(()) }
    async fn find_unsynced_movements(&self, _c: Uuid) -> Result<Vec<WalletMovement>, CoreError> { Ok(vec![]) }
    async fn mark_movement_synced(&self, _c: Uuid, _i: Uuid, _u: NaiveDateTime) -> Result<(), CoreError> { Ok(()) }
    async fn find_movements_updated_since(&self, _c: Uuid, _s: NaiveDateTime) -> Result<Vec<WalletMovement>, CoreError> { Ok(vec![]) }
    async fn sync_upsert_movement(&self, _m: &WalletMovement) -> Result<(), CoreError> { Ok(()) }
}

fn setup() -> (WalletService, Uuid, Uuid) {
    let svc = WalletService::new(Arc::new(MockWalletRepo::new()));
    (svc, Uuid::new_v4(), Uuid::new_v4())
}

#[tokio::test]
async fn deposit_and_withdraw_adjust_balance() {
    let (svc, cid, cust) = setup();
    let acc = svc.open_account(cid, cust, dec!(0)).await.unwrap();
    let (a, _) = svc.deposit(cid, acc.base.id, dec!(100.00), None).await.unwrap();
    assert_eq!(a.balance, dec!(100.00));
    let (a, _) = svc.withdraw(cid, acc.base.id, dec!(30.50), None).await.unwrap();
    assert_eq!(a.balance, dec!(69.50));
}

#[tokio::test]
async fn charge_and_refund_order() {
    let (svc, cid, cust) = setup();
    let acc = svc.open_account(cid, cust, dec!(0)).await.unwrap();
    svc.deposit(cid, acc.base.id, dec!(50), None).await.unwrap();
    let order = Uuid::new_v4();
    let (a, _) = svc.charge_order(cid, acc.base.id, dec!(20.00), order).await.unwrap();
    assert_eq!(a.balance, dec!(30.00));
    let (a, _) = svc.refund_order(cid, acc.base.id, dec!(20.00), order).await.unwrap();
    assert_eq!(a.balance, dec!(50.00));
}

#[tokio::test]
async fn charge_respects_credit_floor() {
    let (svc, cid, cust) = setup();
    // Limite de fiado 50 → saldo pode ir até -50.
    let acc = svc.open_account(cid, cust, dec!(50)).await.unwrap();
    let order = Uuid::new_v4();
    // Cobra 40 sobre saldo 0 → -40 (dentro do limite).
    let (a, _) = svc.charge_order(cid, acc.base.id, dec!(40), order).await.unwrap();
    assert_eq!(a.balance, dec!(-40));
    // Cobrar mais 20 → -60 < -50: rejeitado, saldo intacto.
    let err = svc.charge_order(cid, acc.base.id, dec!(20), order).await;
    assert!(err.is_err(), "cobrança que estoura o fiado deve falhar");
    let a = svc.find_account_by_id(cid, acc.base.id).await.unwrap().unwrap();
    assert_eq!(a.balance, dec!(-40), "saldo não muda após cobrança rejeitada");
}

#[tokio::test]
async fn manual_adjust_allows_negative_correction() {
    let (svc, cid, cust) = setup();
    let acc = svc.open_account(cid, cust, dec!(100)).await.unwrap();
    svc.deposit(cid, acc.base.id, dec!(10), None).await.unwrap();
    // Ajuste negativo (correção) — permitido com justificativa.
    let (a, _) = svc.manual_adjust(cid, acc.base.id, dec!(-4.00), "correção".into()).await.unwrap();
    assert_eq!(a.balance, dec!(6.00));
}
