//! Estorno de cobrança recorrente (cartão e Pix Automático).
//!
//! Um estorno devolve o dinheiro ao pagador — a fatura deixa de estar
//! quitada e a assinatura volta a ser inadimplente. Antes, "refunded" não
//! casava com pago nem com falha e o webhook virava no-op: fatura seguia
//! PAGA, assinatura ativa, e o estorno passava despercebido.
//!
//! O alvo do estorno é a fatura que ESTAVA paga, não a do mês corrente: a
//! devolução chega dias ou meses depois da cobrança.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{NaiveDate, NaiveDateTime};
use rust_decimal_macros::dec;
use uuid::Uuid;

use letaf_core::error::CoreError;
use letaf_core::subscription::model::{
    Invoice, InvoiceStatus, PlanKind, Subscription, SubscriptionStatus,
};
use letaf_core::subscription::repository::SubscriptionRepository;
use letaf_core::subscription::service::SubscriptionService;

struct FakeRepo {
    sub: Mutex<Subscription>,
    invoices: Mutex<Vec<Invoice>>,
}

#[async_trait]
impl SubscriptionRepository for FakeRepo {
    async fn find_current(&self, _company_id: Uuid) -> Result<Option<Subscription>, CoreError> {
        Ok(Some(self.sub.lock().unwrap().clone()))
    }
    async fn find_subscription_by_id(&self, _id: Uuid) -> Result<Option<Subscription>, CoreError> {
        Ok(Some(self.sub.lock().unwrap().clone()))
    }
    async fn update_subscription(&self, s: &Subscription) -> Result<(), CoreError> {
        *self.sub.lock().unwrap() = s.clone();
        Ok(())
    }
    async fn find_invoices(&self, _company_id: Uuid) -> Result<Vec<Invoice>, CoreError> {
        Ok(self.invoices.lock().unwrap().clone())
    }
    async fn create_invoice(&self, inv: &Invoice) -> Result<(), CoreError> {
        self.invoices.lock().unwrap().push(inv.clone());
        Ok(())
    }
    async fn update_invoice(&self, inv: &Invoice) -> Result<(), CoreError> {
        let mut all = self.invoices.lock().unwrap();
        if let Some(slot) = all.iter_mut().find(|i| i.base.id == inv.base.id) {
            *slot = inv.clone();
        }
        Ok(())
    }
    async fn find_invoice_in_month(
        &self,
        subscription_id: Uuid,
        year: i32,
        month: u32,
    ) -> Result<Option<Invoice>, CoreError> {
        use chrono::Datelike;
        Ok(self
            .invoices
            .lock()
            .unwrap()
            .iter()
            .find(|i| {
                i.subscription_id == subscription_id
                    && i.issued_at.year() == year
                    && i.issued_at.month() == month
            })
            .cloned())
    }

    // ── Fora do caminho deste teste ─────────────────────────────────
    async fn create_subscription(&self, _s: &Subscription) -> Result<(), CoreError> {
        unimplemented!()
    }
    async fn find_due_subscriptions(
        &self,
        _today: NaiveDate,
    ) -> Result<Vec<Subscription>, CoreError> {
        unimplemented!()
    }
    async fn find_overdue_candidates(
        &self,
        _today: NaiveDate,
        _grace_days: i64,
    ) -> Result<Vec<Subscription>, CoreError> {
        unimplemented!()
    }
    async fn find_by_gateway_subscription_id(
        &self,
        _id: &str,
    ) -> Result<Option<Subscription>, CoreError> {
        unimplemented!()
    }
    async fn find_by_pix_auto_rec_id(
        &self,
        _rec_id: &str,
    ) -> Result<Option<Subscription>, CoreError> {
        unimplemented!()
    }
    async fn find_unsynced_subscriptions(
        &self,
        _company_id: Uuid,
    ) -> Result<Vec<Subscription>, CoreError> {
        unimplemented!()
    }
    async fn find_unsynced_invoices(&self, _company_id: Uuid) -> Result<Vec<Invoice>, CoreError> {
        unimplemented!()
    }
    async fn mark_subscription_synced(
        &self,
        _company_id: Uuid,
        _id: Uuid,
        _updated_at: NaiveDateTime,
    ) -> Result<(), CoreError> {
        unimplemented!()
    }
    async fn mark_invoice_synced(
        &self,
        _company_id: Uuid,
        _id: Uuid,
        _updated_at: NaiveDateTime,
    ) -> Result<(), CoreError> {
        unimplemented!()
    }
    async fn sync_upsert_subscription(&self, _s: &Subscription) -> Result<(), CoreError> {
        unimplemented!()
    }
    async fn sync_upsert_invoice(&self, _inv: &Invoice) -> Result<(), CoreError> {
        unimplemented!()
    }
    async fn find_subscriptions_updated_since(
        &self,
        _company_id: Uuid,
        _since: NaiveDateTime,
    ) -> Result<Vec<Subscription>, CoreError> {
        unimplemented!()
    }
    async fn find_invoices_updated_since(
        &self,
        _company_id: Uuid,
        _since: NaiveDateTime,
    ) -> Result<Vec<Invoice>, CoreError> {
        unimplemented!()
    }
}

fn dia(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

fn fatura_paga(
    company_id: Uuid,
    subscription_id: Uuid,
    numero: &str,
    emitida: NaiveDate,
) -> Invoice {
    Invoice::new(
        company_id,
        subscription_id,
        numero.into(),
        "Assinatura".into(),
        dec!(200),
        "card".into(),
        "VISA •••• 4242".into(),
        InvoiceStatus::Paid,
        emitida,
        Some(emitida.and_hms_opt(10, 0, 0).unwrap()),
    )
}

/// Assinatura ativa com faturas pagas em maio e junho; "hoje" é agosto.
fn cenario() -> (Arc<FakeRepo>, SubscriptionService, Subscription) {
    let company_id = Uuid::new_v4();
    let mut sub = Subscription::new(company_id, PlanKind::Monthly);
    sub.status = SubscriptionStatus::Active;
    let maio = fatura_paga(company_id, sub.base.id, "NFS-0001", dia(2026, 5, 10));
    let junho = fatura_paga(company_id, sub.base.id, "NFS-0002", dia(2026, 6, 10));
    let repo = Arc::new(FakeRepo {
        sub: Mutex::new(sub.clone()),
        invoices: Mutex::new(vec![maio, junho]),
    });
    let svc = SubscriptionService::new(repo.clone());
    (repo, svc, sub)
}

#[tokio::test]
async fn estorno_desfaz_a_fatura_mais_recente_e_deixa_a_assinatura_inadimplente() {
    let (repo, svc, sub) = cenario();

    svc.apply_card_charge(&sub, "refunded", dec!(200), None, dia(2026, 8, 1))
        .await
        .unwrap();

    let faturas = repo.invoices.lock().unwrap().clone();
    assert_eq!(faturas.len(), 2, "estorno não pode criar fatura nova");

    let junho = faturas.iter().find(|i| i.number == "NFS-0002").unwrap();
    assert_eq!(
        junho.status,
        InvoiceStatus::Failed,
        "a fatura estornada é a mais recente que estava paga"
    );
    assert!(
        junho.paid_at.is_none(),
        "fatura estornada não pode manter data de pagamento — o dinheiro voltou"
    );

    let maio = faturas.iter().find(|i| i.number == "NFS-0001").unwrap();
    assert_eq!(maio.status, InvoiceStatus::Paid, "só uma fatura foi estornada");

    assert_eq!(
        repo.sub.lock().unwrap().status,
        SubscriptionStatus::Overdue,
        "com o dinheiro devolvido, a assinatura volta a ser inadimplente"
    );
}

#[tokio::test]
async fn chargeback_e_contestacao_valem_como_estorno() {
    for status in ["chargeback", "contested", "estornado", "devolvido"] {
        let (repo, svc, sub) = cenario();
        svc.apply_card_charge(&sub, status, dec!(200), None, dia(2026, 8, 1))
            .await
            .unwrap();
        let faturas = repo.invoices.lock().unwrap().clone();
        assert_eq!(faturas.len(), 2, "{status} não pode criar fatura nova");
        let junho = faturas.iter().find(|i| i.number == "NFS-0002").unwrap();
        assert_eq!(junho.status, InvoiceStatus::Failed, "status: {status}");
    }
}

#[tokio::test]
async fn estorno_sem_fatura_paga_nao_inventa_cobranca() {
    let company_id = Uuid::new_v4();
    let sub = Subscription::new(company_id, PlanKind::Monthly);
    let repo = Arc::new(FakeRepo {
        sub: Mutex::new(sub.clone()),
        invoices: Mutex::new(Vec::new()),
    });
    let svc = SubscriptionService::new(repo.clone());

    svc.apply_card_charge(&sub, "refunded", dec!(200), None, dia(2026, 8, 1))
        .await
        .unwrap();

    assert!(
        repo.invoices.lock().unwrap().is_empty(),
        "sem fatura paga não há o que estornar — não pode aparecer cobrança do nada"
    );
}

/// O caminho feliz não pode ter sido afetado: uma cobrança APROVADA no mês
/// corrente segue criando/quitando a fatura do ciclo.
#[tokio::test]
async fn pagamento_aprovado_continua_quitando_o_ciclo_corrente() {
    let (repo, svc, sub) = cenario();

    svc.apply_card_charge(&sub, "paid", dec!(200), None, dia(2026, 8, 1))
        .await
        .unwrap();

    let faturas = repo.invoices.lock().unwrap().clone();
    assert_eq!(faturas.len(), 3, "cria a fatura de agosto");
    let agosto = faturas.last().unwrap();
    assert_eq!(agosto.status, InvoiceStatus::Paid);
    assert!(agosto.paid_at.is_some());
}
