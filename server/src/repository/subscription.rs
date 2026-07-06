use async_trait::async_trait;
use chrono::{NaiveDate, NaiveDateTime};
use sqlx::prelude::FromRow;
use sqlx::PgPool;
use uuid::Uuid;

use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;
use letaf_core::subscription::model::{
    Invoice, InvoiceStatus, PaymentMethod, Subscription, SubscriptionStatus,
};
use letaf_core::subscription::model::PlanKind;
use letaf_core::subscription::repository::SubscriptionRepository;

use super::helpers::map_db;

#[derive(FromRow)]
struct SubscriptionRow {
    id: Uuid,
    company_id: Uuid,
    plan_kind: String,
    next_charge_date: Option<NaiveDate>,
    status: String,
    payment_method_kind: String,
    payment_method_label: String,
    payment_method_expiry: String,
    gateway: Option<String>,
    gateway_subscription_id: Option<String>,
    card_status: Option<String>,
    pix_auto_rec_id: Option<String>,
    pix_auto_status: Option<String>,
    plan_id: Option<Uuid>,
    plan_name: String,
    plan_amount: f64,
    plan_period_months: i32,
    trial_days: i32,
    plan_discount_monthly: f64,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    deleted_at: Option<NaiveDateTime>,
    synced: bool,
}

impl From<SubscriptionRow> for Subscription {
    fn from(r: SubscriptionRow) -> Self {
        Self {
            base: BaseFields {
                id: r.id,
                company_id: r.company_id,
                created_at: r.created_at,
                updated_at: r.updated_at,
                deleted_at: r.deleted_at,
                synced: r.synced,
            },
            plan_kind: PlanKind::from_str(&r.plan_kind),
            next_charge_date: r.next_charge_date,
            status: SubscriptionStatus::from_str(&r.status),
            payment_method: PaymentMethod {
                kind: r.payment_method_kind,
                label: r.payment_method_label,
                expiry: r.payment_method_expiry,
            },
            gateway: r.gateway,
            gateway_subscription_id: r.gateway_subscription_id,
            card_status: r.card_status,
            pix_auto_rec_id: r.pix_auto_rec_id,
            pix_auto_status: r.pix_auto_status,
            plan_id: r.plan_id,
            plan_name: r.plan_name,
            plan_amount: r.plan_amount,
            plan_period_months: r.plan_period_months,
            trial_days: r.trial_days,
            plan_discount_monthly: r.plan_discount_monthly,
        }
    }
}

#[derive(FromRow)]
struct InvoiceRow {
    id: Uuid,
    company_id: Uuid,
    subscription_id: Uuid,
    number: String,
    description: String,
    amount: f64,
    method_kind: String,
    method_label: String,
    status: String,
    issued_at: NaiveDate,
    paid_at: Option<NaiveDateTime>,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    deleted_at: Option<NaiveDateTime>,
    synced: bool,
}

impl From<InvoiceRow> for Invoice {
    fn from(r: InvoiceRow) -> Self {
        Self {
            base: BaseFields {
                id: r.id,
                company_id: r.company_id,
                created_at: r.created_at,
                updated_at: r.updated_at,
                deleted_at: r.deleted_at,
                synced: r.synced,
            },
            subscription_id: r.subscription_id,
            number: r.number,
            description: r.description,
            amount: r.amount,
            method_kind: r.method_kind,
            method_label: r.method_label,
            status: InvoiceStatus::from_str(&r.status),
            issued_at: r.issued_at,
            paid_at: r.paid_at,
        }
    }
}

pub struct PgSubscriptionRepository {
    pool: PgPool,
}

impl PgSubscriptionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SubscriptionRepository for PgSubscriptionRepository {
    async fn find_subscription_by_id(
        &self,
        id: Uuid,
    ) -> Result<Option<Subscription>, CoreError> {
        sqlx::query_as::<_, SubscriptionRow>(
            "SELECT * FROM subscriptions WHERE id = $1 AND deleted_at IS NULL LIMIT 1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map(|opt| opt.map(Subscription::from))
        .map_err(map_db)
    }

    async fn find_current(
        &self,
        company_id: Uuid,
    ) -> Result<Option<Subscription>, CoreError> {
        sqlx::query_as::<_, SubscriptionRow>(
            "SELECT * FROM subscriptions
             WHERE company_id = $1 AND deleted_at IS NULL
             ORDER BY created_at DESC
             LIMIT 1",
        )
        .bind(company_id)
        .fetch_optional(&self.pool)
        .await
        .map(|opt| opt.map(Subscription::from))
        .map_err(map_db)
    }

    async fn create_subscription(&self, s: &Subscription) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO subscriptions
             (id, company_id, plan_kind, next_charge_date, status,
              payment_method_kind, payment_method_label, payment_method_expiry,
              gateway, gateway_subscription_id, card_status,
              pix_auto_rec_id, pix_auto_status,
              plan_id, plan_name, plan_amount, plan_period_months, trial_days,
              plan_discount_monthly,
              created_at, updated_at, deleted_at, synced)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23)",
        )
        .bind(s.base.id)
        .bind(s.base.company_id)
        .bind(s.plan_kind.as_str())
        .bind(s.next_charge_date)
        .bind(s.status.as_str())
        .bind(&s.payment_method.kind)
        .bind(&s.payment_method.label)
        .bind(&s.payment_method.expiry)
        .bind(&s.gateway)
        .bind(&s.gateway_subscription_id)
        .bind(&s.card_status)
        .bind(&s.pix_auto_rec_id)
        .bind(&s.pix_auto_status)
        .bind(s.plan_id)
        .bind(&s.plan_name)
        .bind(s.plan_amount)
        .bind(s.plan_period_months)
        .bind(s.trial_days)
        .bind(s.plan_discount_monthly)
        .bind(s.base.created_at)
        .bind(s.base.updated_at)
        .bind(s.base.deleted_at)
        .bind(s.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn update_subscription(&self, s: &Subscription) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE subscriptions
                SET plan_kind = $1, next_charge_date = $2, status = $3,
                    payment_method_kind = $4, payment_method_label = $5,
                    payment_method_expiry = $6, gateway = $7,
                    gateway_subscription_id = $8, card_status = $9,
                    pix_auto_rec_id = $10, pix_auto_status = $11,
                    plan_id = $12, plan_name = $13, plan_amount = $14,
                    plan_period_months = $15, trial_days = $16,
                    plan_discount_monthly = $17,
                    updated_at = $18, synced = $19
              WHERE company_id = $20 AND id = $21 AND deleted_at IS NULL",
        )
        .bind(s.plan_kind.as_str())
        .bind(s.next_charge_date)
        .bind(s.status.as_str())
        .bind(&s.payment_method.kind)
        .bind(&s.payment_method.label)
        .bind(&s.payment_method.expiry)
        .bind(&s.gateway)
        .bind(&s.gateway_subscription_id)
        .bind(&s.card_status)
        .bind(&s.pix_auto_rec_id)
        .bind(&s.pix_auto_status)
        .bind(s.plan_id)
        .bind(&s.plan_name)
        .bind(s.plan_amount)
        .bind(s.plan_period_months)
        .bind(s.trial_days)
        .bind(s.plan_discount_monthly)
        .bind(s.base.updated_at)
        .bind(s.base.synced)
        .bind(s.base.company_id)
        .bind(s.base.id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn find_invoices(&self, company_id: Uuid) -> Result<Vec<Invoice>, CoreError> {
        let rows = sqlx::query_as::<_, InvoiceRow>(
            "SELECT * FROM subscription_invoices
             WHERE company_id = $1 AND deleted_at IS NULL
             ORDER BY issued_at DESC",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(Invoice::from).collect())
    }

    async fn create_invoice(&self, inv: &Invoice) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO subscription_invoices
             (id, company_id, subscription_id, number, description, amount,
              method_kind, method_label, status, issued_at, paid_at,
              created_at, updated_at, deleted_at, synced)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)",
        )
        .bind(inv.base.id)
        .bind(inv.base.company_id)
        .bind(inv.subscription_id)
        .bind(&inv.number)
        .bind(&inv.description)
        .bind(inv.amount)
        .bind(&inv.method_kind)
        .bind(&inv.method_label)
        .bind(inv.status.as_str())
        .bind(inv.issued_at)
        .bind(inv.paid_at)
        .bind(inv.base.created_at)
        .bind(inv.base.updated_at)
        .bind(inv.base.deleted_at)
        .bind(inv.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn update_invoice(&self, inv: &Invoice) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE subscription_invoices
                SET subscription_id = $1, number = $2, description = $3, amount = $4,
                    method_kind = $5, method_label = $6, status = $7, issued_at = $8,
                    paid_at = $9, updated_at = $10, synced = $11
              WHERE company_id = $12 AND id = $13 AND deleted_at IS NULL",
        )
        .bind(inv.subscription_id)
        .bind(&inv.number)
        .bind(&inv.description)
        .bind(inv.amount)
        .bind(&inv.method_kind)
        .bind(&inv.method_label)
        .bind(inv.status.as_str())
        .bind(inv.issued_at)
        .bind(inv.paid_at)
        .bind(inv.base.updated_at)
        .bind(inv.base.synced)
        .bind(inv.base.company_id)
        .bind(inv.base.id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn find_due_subscriptions(
        &self,
        today: chrono::NaiveDate,
    ) -> Result<Vec<Subscription>, CoreError> {
        let rows = sqlx::query_as::<_, SubscriptionRow>(
            "SELECT * FROM subscriptions
             WHERE deleted_at IS NULL
               AND status = 'active'
               AND next_charge_date IS NOT NULL
               AND next_charge_date <= $1",
        )
        .bind(today)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(Subscription::from).collect())
    }

    async fn find_overdue_candidates(
        &self,
        today: chrono::NaiveDate,
        grace_days: i64,
    ) -> Result<Vec<Subscription>, CoreError> {
        // Assinaturas ativas que têm pelo menos uma invoice Pending
        // com `issued_at + grace_days < today`.
        let rows = sqlx::query_as::<_, SubscriptionRow>(
            "SELECT s.* FROM subscriptions s
             WHERE s.deleted_at IS NULL
               AND s.status = 'active'
               AND EXISTS (
                 SELECT 1 FROM subscription_invoices i
                 WHERE i.subscription_id = s.id
                   AND i.deleted_at IS NULL
                   AND i.status = 'pending'
                   AND i.issued_at + ($2 || ' days')::interval < $1
               )",
        )
        .bind(today)
        .bind(grace_days.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(Subscription::from).collect())
    }

    async fn find_by_gateway_subscription_id(
        &self,
        gateway_subscription_id: &str,
    ) -> Result<Option<Subscription>, CoreError> {
        sqlx::query_as::<_, SubscriptionRow>(
            "SELECT * FROM subscriptions
             WHERE gateway_subscription_id = $1 AND deleted_at IS NULL
             LIMIT 1",
        )
        .bind(gateway_subscription_id)
        .fetch_optional(&self.pool)
        .await
        .map(|opt| opt.map(Subscription::from))
        .map_err(map_db)
    }

    async fn find_by_pix_auto_rec_id(
        &self,
        rec_id: &str,
    ) -> Result<Option<Subscription>, CoreError> {
        sqlx::query_as::<_, SubscriptionRow>(
            "SELECT * FROM subscriptions
             WHERE pix_auto_rec_id = $1 AND deleted_at IS NULL
             LIMIT 1",
        )
        .bind(rec_id)
        .fetch_optional(&self.pool)
        .await
        .map(|opt| opt.map(Subscription::from))
        .map_err(map_db)
    }

    async fn find_invoice_in_month(
        &self,
        subscription_id: Uuid,
        year: i32,
        month: u32,
    ) -> Result<Option<Invoice>, CoreError> {
        let start = chrono::NaiveDate::from_ymd_opt(year, month, 1)
            .ok_or_else(|| CoreError::Validation("Data inválida".into()))?;
        let end = chrono::NaiveDate::from_ymd_opt(
            year + (month / 12) as i32,
            (month % 12) + 1,
            1,
        )
        .ok_or_else(|| CoreError::Validation("Data inválida".into()))?;
        sqlx::query_as::<_, InvoiceRow>(
            "SELECT * FROM subscription_invoices
             WHERE subscription_id = $1
               AND deleted_at IS NULL
               AND issued_at >= $2
               AND issued_at < $3
             ORDER BY issued_at DESC
             LIMIT 1",
        )
        .bind(subscription_id)
        .bind(start)
        .bind(end)
        .fetch_optional(&self.pool)
        .await
        .map(|opt| opt.map(Invoice::from))
        .map_err(map_db)
    }

    async fn find_unsynced_subscriptions(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<Subscription>, CoreError> {
        let rows = sqlx::query_as::<_, SubscriptionRow>(
            "SELECT * FROM subscriptions WHERE company_id = $1 AND synced = FALSE",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(Subscription::from).collect())
    }

    async fn find_unsynced_invoices(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<Invoice>, CoreError> {
        let rows = sqlx::query_as::<_, InvoiceRow>(
            "SELECT * FROM subscription_invoices WHERE company_id = $1 AND synced = FALSE",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(Invoice::from).collect())
    }

    async fn mark_subscription_synced(
        &self,
        company_id: Uuid,
        id: Uuid,
        updated_at: chrono::NaiveDateTime,
    ) -> Result<(), CoreError> {
        sqlx::query("UPDATE subscriptions SET synced = TRUE WHERE company_id = $1 AND id = $2 AND updated_at = $3")
            .bind(company_id)
            .bind(id)
        .bind(updated_at)
            .execute(&self.pool)
            .await
            .map_err(map_db)?;
        Ok(())
    }

    async fn mark_invoice_synced(
        &self,
        company_id: Uuid,
        id: Uuid,
        updated_at: chrono::NaiveDateTime,
    ) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE subscription_invoices SET synced = TRUE WHERE company_id = $1 AND id = $2 AND updated_at = $3",
        )
        .bind(company_id)
        .bind(id)
        .bind(updated_at)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn sync_upsert_subscription(&self, s: &Subscription) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO subscriptions
             (id, company_id, plan_kind, next_charge_date, status,
              payment_method_kind, payment_method_label, payment_method_expiry,
              gateway, gateway_subscription_id, card_status,
              pix_auto_rec_id, pix_auto_status,
              plan_id, plan_name, plan_amount, plan_period_months, trial_days,
              plan_discount_monthly,
              created_at, updated_at, deleted_at, synced)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23)
             ON CONFLICT (id) DO UPDATE SET
                plan_kind = EXCLUDED.plan_kind,
                next_charge_date = EXCLUDED.next_charge_date,
                status = EXCLUDED.status,
                payment_method_kind = EXCLUDED.payment_method_kind,
                payment_method_label = EXCLUDED.payment_method_label,
                payment_method_expiry = EXCLUDED.payment_method_expiry,
                gateway = EXCLUDED.gateway,
                gateway_subscription_id = EXCLUDED.gateway_subscription_id,
                card_status = EXCLUDED.card_status,
                pix_auto_rec_id = EXCLUDED.pix_auto_rec_id,
                pix_auto_status = EXCLUDED.pix_auto_status,
                plan_id = EXCLUDED.plan_id,
                plan_name = EXCLUDED.plan_name,
                plan_amount = EXCLUDED.plan_amount,
                plan_period_months = EXCLUDED.plan_period_months,
                trial_days = EXCLUDED.trial_days,
                plan_discount_monthly = EXCLUDED.plan_discount_monthly,
                updated_at = EXCLUDED.updated_at,
                deleted_at = EXCLUDED.deleted_at,
                synced = EXCLUDED.synced
             WHERE EXCLUDED.updated_at > subscriptions.updated_at AND subscriptions.company_id = EXCLUDED.company_id",
        )
        .bind(s.base.id)
        .bind(s.base.company_id)
        .bind(s.plan_kind.as_str())
        .bind(s.next_charge_date)
        .bind(s.status.as_str())
        .bind(&s.payment_method.kind)
        .bind(&s.payment_method.label)
        .bind(&s.payment_method.expiry)
        .bind(&s.gateway)
        .bind(&s.gateway_subscription_id)
        .bind(&s.card_status)
        .bind(&s.pix_auto_rec_id)
        .bind(&s.pix_auto_status)
        .bind(s.plan_id)
        .bind(&s.plan_name)
        .bind(s.plan_amount)
        .bind(s.plan_period_months)
        .bind(s.trial_days)
        .bind(s.plan_discount_monthly)
        .bind(s.base.created_at)
        .bind(s.base.updated_at)
        .bind(s.base.deleted_at)
        .bind(s.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn sync_upsert_invoice(&self, inv: &Invoice) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO subscription_invoices
             (id, company_id, subscription_id, number, description, amount,
              method_kind, method_label, status, issued_at, paid_at,
              created_at, updated_at, deleted_at, synced)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)
             ON CONFLICT (id) DO UPDATE SET
                subscription_id = EXCLUDED.subscription_id,
                number = EXCLUDED.number,
                description = EXCLUDED.description,
                amount = EXCLUDED.amount,
                method_kind = EXCLUDED.method_kind,
                method_label = EXCLUDED.method_label,
                status = EXCLUDED.status,
                issued_at = EXCLUDED.issued_at,
                paid_at = EXCLUDED.paid_at,
                updated_at = EXCLUDED.updated_at,
                deleted_at = EXCLUDED.deleted_at,
                synced = EXCLUDED.synced
             WHERE EXCLUDED.updated_at > subscription_invoices.updated_at AND subscription_invoices.company_id = EXCLUDED.company_id",
        )
        .bind(inv.base.id)
        .bind(inv.base.company_id)
        .bind(inv.subscription_id)
        .bind(&inv.number)
        .bind(&inv.description)
        .bind(inv.amount)
        .bind(&inv.method_kind)
        .bind(&inv.method_label)
        .bind(inv.status.as_str())
        .bind(inv.issued_at)
        .bind(inv.paid_at)
        .bind(inv.base.created_at)
        .bind(inv.base.updated_at)
        .bind(inv.base.deleted_at)
        .bind(inv.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn find_subscriptions_updated_since(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
    ) -> Result<Vec<Subscription>, CoreError> {
        let rows = sqlx::query_as::<_, SubscriptionRow>(
            "SELECT * FROM subscriptions WHERE company_id = $1 AND updated_at > $2",
        )
        .bind(company_id)
        .bind(since)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(Subscription::from).collect())
    }

    async fn find_invoices_updated_since(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
    ) -> Result<Vec<Invoice>, CoreError> {
        let rows = sqlx::query_as::<_, InvoiceRow>(
            "SELECT * FROM subscription_invoices WHERE company_id = $1 AND updated_at > $2",
        )
        .bind(company_id)
        .bind(since)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(Invoice::from).collect())
    }
}
