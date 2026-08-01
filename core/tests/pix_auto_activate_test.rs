//! Ativação do Pix Automático — o mandato ANTERIOR precisa morrer antes de
//! nascer o novo.
//!
//! Cada recorrência autoriza o banco do pagador a debitar sozinho. Duas
//! recorrências vivas para a mesma assinatura = dois débitos no mesmo ciclo,
//! sem nada denunciando o problema (o `custom_id` é único por recorrência,
//! então a Efi aceita a criação numa boa). Este teste tranca a ordem:
//! `cancel_recurrence(anterior)` antes de `create_recurrence`.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{NaiveDate, NaiveDateTime};
use uuid::Uuid;

use letaf_core::error::CoreError;
use letaf_core::payment_gateway::pix_auto::{
    CreatedRecurrence, PixAutoChargeEvent, PixAutoGateway, PixAutoInput, RecurrenceStatus,
};
use letaf_core::subscription::model::{Invoice, PlanKind, Subscription};
use letaf_core::subscription::pix_auto_billing::PixAutoBillingService;
use letaf_core::subscription::repository::SubscriptionRepository;
use letaf_core::subscription::service::SubscriptionService;

/// Passos observados no gateway, na ordem em que aconteceram.
type Trilha = Arc<Mutex<Vec<String>>>;

struct FakeGateway {
    trilha: Trilha,
}

#[async_trait]
impl PixAutoGateway for FakeGateway {
    async fn create_recurrence(
        &self,
        _input: &PixAutoInput,
    ) -> Result<CreatedRecurrence, CoreError> {
        self.trilha.lock().unwrap().push("create".into());
        Ok(CreatedRecurrence {
            rec_id: "rec-novo".into(),
            copia_cola: "cc".into(),
            qr_code_b64: String::new(),
            status: "pending".into(),
        })
    }

    async fn fetch_recurrence_status(&self, _rec_id: &str) -> Result<RecurrenceStatus, CoreError> {
        unimplemented!("não usado neste teste")
    }

    async fn create_recurring_charge(
        &self,
        _rec_id: &str,
        _amount_cents: i64,
        _due_date: NaiveDate,
        _description: &str,
        _custom_id: &str,
    ) -> Result<(), CoreError> {
        unimplemented!("não usado neste teste")
    }

    async fn cancel_recurrence(&self, rec_id: &str) -> Result<(), CoreError> {
        self.trilha.lock().unwrap().push(format!("cancel:{rec_id}"));
        Ok(())
    }

    fn parse_webhook(&self, _body: &str) -> Result<Vec<PixAutoChargeEvent>, CoreError> {
        unimplemented!("não usado neste teste")
    }

    fn name(&self) -> &'static str {
        "fake"
    }
}

/// Gateway que recusa o cancelamento — simula Efi fora do ar ou mandato já
/// encerrado do lado de lá.
struct GatewayQueNaoCancela {
    trilha: Trilha,
}

#[async_trait]
impl PixAutoGateway for GatewayQueNaoCancela {
    async fn create_recurrence(
        &self,
        _input: &PixAutoInput,
    ) -> Result<CreatedRecurrence, CoreError> {
        self.trilha.lock().unwrap().push("create".into());
        Ok(CreatedRecurrence {
            rec_id: "rec-novo".into(),
            copia_cola: "cc".into(),
            qr_code_b64: String::new(),
            status: "pending".into(),
        })
    }
    async fn fetch_recurrence_status(&self, _rec_id: &str) -> Result<RecurrenceStatus, CoreError> {
        unimplemented!()
    }
    async fn create_recurring_charge(
        &self,
        _rec_id: &str,
        _amount_cents: i64,
        _due_date: NaiveDate,
        _description: &str,
        _custom_id: &str,
    ) -> Result<(), CoreError> {
        unimplemented!()
    }
    async fn cancel_recurrence(&self, rec_id: &str) -> Result<(), CoreError> {
        self.trilha.lock().unwrap().push(format!("cancel-falhou:{rec_id}"));
        Err(CoreError::Repository("gateway fora do ar".into()))
    }
    fn parse_webhook(&self, _body: &str) -> Result<Vec<PixAutoChargeEvent>, CoreError> {
        unimplemented!()
    }
    fn name(&self) -> &'static str {
        "fake"
    }
}

/// Repositório em memória com só o que `activate` toca.
struct FakeRepo {
    atual: Mutex<Subscription>,
}

#[async_trait]
impl SubscriptionRepository for FakeRepo {
    async fn find_current(&self, _company_id: Uuid) -> Result<Option<Subscription>, CoreError> {
        Ok(Some(self.atual.lock().unwrap().clone()))
    }
    async fn update_subscription(&self, s: &Subscription) -> Result<(), CoreError> {
        *self.atual.lock().unwrap() = s.clone();
        Ok(())
    }

    // ── Fora do caminho de `activate` ───────────────────────────────
    async fn find_subscription_by_id(&self, _id: Uuid) -> Result<Option<Subscription>, CoreError> {
        unimplemented!()
    }
    async fn create_subscription(&self, _s: &Subscription) -> Result<(), CoreError> {
        unimplemented!()
    }
    async fn find_invoices(&self, _company_id: Uuid) -> Result<Vec<Invoice>, CoreError> {
        unimplemented!()
    }
    async fn create_invoice(&self, _inv: &Invoice) -> Result<(), CoreError> {
        unimplemented!()
    }
    async fn update_invoice(&self, _inv: &Invoice) -> Result<(), CoreError> {
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
    async fn find_invoice_in_month(
        &self,
        _subscription_id: Uuid,
        _year: i32,
        _month: u32,
    ) -> Result<Option<Invoice>, CoreError> {
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

fn monta(
    rec_anterior: Option<&str>,
    gateway: Arc<dyn PixAutoGateway>,
) -> (PixAutoBillingService, Uuid) {
    let company_id = Uuid::new_v4();
    let mut sub = Subscription::new(company_id, PlanKind::Monthly);
    sub.pix_auto_rec_id = rec_anterior.map(str::to_string);
    let repo = Arc::new(FakeRepo {
        atual: Mutex::new(sub),
    });
    let subscriptions = Arc::new(SubscriptionService::new(repo));
    (
        PixAutoBillingService::new(gateway, subscriptions, "https://exemplo/webhook".into()),
        company_id,
    )
}

#[tokio::test]
async fn revoga_o_mandato_anterior_antes_de_criar_o_novo() {
    let trilha: Trilha = Arc::new(Mutex::new(Vec::new()));
    let gateway = Arc::new(FakeGateway {
        trilha: trilha.clone(),
    });
    let (billing, company_id) = monta(Some("rec-antigo"), gateway);

    let (sub, criada) = billing
        .activate(company_id, "Fulano".into(), "12345678901".into())
        .await
        .unwrap();

    assert_eq!(
        *trilha.lock().unwrap(),
        vec!["cancel:rec-antigo".to_string(), "create".to_string()],
        "o mandato anterior precisa ser cancelado ANTES de criar o novo — \
         dois mandatos vivos debitam o cliente duas vezes"
    );
    assert_eq!(criada.rec_id, "rec-novo");
    assert_eq!(sub.pix_auto_rec_id.as_deref(), Some("rec-novo"));
}

#[tokio::test]
async fn primeira_ativacao_nao_tenta_cancelar_nada() {
    let trilha: Trilha = Arc::new(Mutex::new(Vec::new()));
    let gateway = Arc::new(FakeGateway {
        trilha: trilha.clone(),
    });
    let (billing, company_id) = monta(None, gateway);

    billing
        .activate(company_id, "Fulano".into(), "12345678901".into())
        .await
        .unwrap();

    assert_eq!(
        *trilha.lock().unwrap(),
        vec!["create".to_string()],
        "sem mandato anterior não há o que revogar"
    );
}

#[tokio::test]
async fn falha_ao_revogar_nao_impede_a_ativacao() {
    // O gateway pode estar fora, ou o mandato já ter sido cancelado lá.
    // Travar a assinatura do lojista por causa disso seria pior — o log
    // registra para conferência manual e a ativação segue.
    let trilha: Trilha = Arc::new(Mutex::new(Vec::new()));
    let gateway = Arc::new(GatewayQueNaoCancela {
        trilha: trilha.clone(),
    });
    let (billing, company_id) = monta(Some("rec-antigo"), gateway);

    let (sub, _) = billing
        .activate(company_id, "Fulano".into(), "12345678901".into())
        .await
        .expect("ativação não pode falhar por causa da revogação");

    assert_eq!(
        *trilha.lock().unwrap(),
        vec!["cancel-falhou:rec-antigo".to_string(), "create".to_string()]
    );
    assert_eq!(sub.pix_auto_rec_id.as_deref(), Some("rec-novo"));
}

#[tokio::test]
async fn cpf_invalido_nem_chega_no_gateway() {
    let trilha: Trilha = Arc::new(Mutex::new(Vec::new()));
    let gateway = Arc::new(FakeGateway {
        trilha: trilha.clone(),
    });
    let (billing, company_id) = monta(Some("rec-antigo"), gateway);

    let erro = billing
        .activate(company_id, "Fulano".into(), "123".into())
        .await;

    assert!(erro.is_err());
    assert!(
        trilha.lock().unwrap().is_empty(),
        "validação vem antes: não pode revogar o mandato bom por causa de um CPF inválido"
    );
}
