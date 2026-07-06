use async_trait::async_trait;
use sqlx::prelude::FromRow;
use sqlx::SqlitePool;
use uuid::Uuid;

use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;
use letaf_core::subscription::model::{
    Invoice, InvoiceStatus, PaymentMethod, PlanKind, Subscription, SubscriptionStatus,
};
use letaf_core::subscription::repository::SubscriptionRepository;

use super::helpers::{date_str, map_db, parse_date, parse_timestamp, parse_uuid, ts};

#[derive(FromRow)]
struct SubscriptionRow {
    id: String,
    company_id: String,
    plan_kind: String,
    next_charge_date: Option<String>,
    status: String,
    payment_method_kind: String,
    payment_method_label: String,
    payment_method_expiry: String,
    gateway: Option<String>,
    gateway_subscription_id: Option<String>,
    card_status: Option<String>,
    pix_auto_rec_id: Option<String>,
    pix_auto_status: Option<String>,
    plan_id: Option<String>,
    plan_name: String,
    plan_amount: f64,
    plan_period_months: i64,
    trial_days: i64,
    plan_discount_monthly: f64,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    synced: bool,
}

impl TryFrom<SubscriptionRow> for Subscription {
    type Error = CoreError;

    fn try_from(r: SubscriptionRow) -> Result<Self, Self::Error> {
        Ok(Self {
            base: BaseFields {
                id: parse_uuid(&r.id)?,
                company_id: parse_uuid(&r.company_id)?,
                created_at: parse_timestamp(&r.created_at)?,
                updated_at: parse_timestamp(&r.updated_at)?,
                deleted_at: r.deleted_at.as_deref().map(parse_timestamp).transpose()?,
                synced: r.synced,
            },
            plan_kind: PlanKind::from_str(&r.plan_kind),
            next_charge_date: r.next_charge_date.as_deref().map(parse_date).transpose()?,
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
            plan_id: r.plan_id.as_deref().map(parse_uuid).transpose()?,
            plan_name: r.plan_name,
            plan_amount: r.plan_amount,
            plan_period_months: r.plan_period_months as i32,
            trial_days: r.trial_days as i32,
            plan_discount_monthly: r.plan_discount_monthly,
        })
    }
}

#[derive(FromRow)]
struct InvoiceRow {
    id: String,
    company_id: String,
    subscription_id: String,
    number: String,
    description: String,
    amount: f64,
    method_kind: String,
    method_label: String,
    status: String,
    issued_at: String,
    paid_at: Option<String>,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    synced: bool,
}

impl TryFrom<InvoiceRow> for Invoice {
    type Error = CoreError;

    fn try_from(r: InvoiceRow) -> Result<Self, Self::Error> {
        Ok(Self {
            base: BaseFields {
                id: parse_uuid(&r.id)?,
                company_id: parse_uuid(&r.company_id)?,
                created_at: parse_timestamp(&r.created_at)?,
                updated_at: parse_timestamp(&r.updated_at)?,
                deleted_at: r.deleted_at.as_deref().map(parse_timestamp).transpose()?,
                synced: r.synced,
            },
            subscription_id: parse_uuid(&r.subscription_id)?,
            number: r.number,
            description: r.description,
            amount: r.amount,
            method_kind: r.method_kind,
            method_label: r.method_label,
            status: InvoiceStatus::from_str(&r.status),
            issued_at: parse_date(&r.issued_at)?,
            paid_at: r.paid_at.as_deref().map(parse_timestamp).transpose()?,
        })
    }
}

pub struct SqliteSubscriptionRepository {
    pool: SqlitePool,
}

impl SqliteSubscriptionRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SubscriptionRepository for SqliteSubscriptionRepository {
    async fn find_subscription_by_id(
        &self,
        id: Uuid,
    ) -> Result<Option<Subscription>, CoreError> {
        let row = sqlx::query_as::<_, SubscriptionRow>(
            "SELECT * FROM subscriptions WHERE id = ?1 AND deleted_at IS NULL LIMIT 1",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        row.map(Subscription::try_from).transpose()
    }

    async fn find_current(
        &self,
        company_id: Uuid,
    ) -> Result<Option<Subscription>, CoreError> {
        let row = sqlx::query_as::<_, SubscriptionRow>(
            "SELECT * FROM subscriptions
             WHERE company_id = ?1 AND deleted_at IS NULL
             ORDER BY created_at DESC
             LIMIT 1",
        )
        .bind(company_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        row.map(Subscription::try_from).transpose()
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
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23)",
        )
        .bind(s.base.id.to_string())
        .bind(s.base.company_id.to_string())
        .bind(s.plan_kind.as_str())
        .bind(s.next_charge_date.map(date_str))
        .bind(s.status.as_str())
        .bind(&s.payment_method.kind)
        .bind(&s.payment_method.label)
        .bind(&s.payment_method.expiry)
        .bind(&s.gateway)
        .bind(&s.gateway_subscription_id)
        .bind(&s.card_status)
        .bind(&s.pix_auto_rec_id)
        .bind(&s.pix_auto_status)
        .bind(s.plan_id.map(|id| id.to_string()))
        .bind(&s.plan_name)
        .bind(s.plan_amount)
        .bind(s.plan_period_months as i64)
        .bind(s.trial_days as i64)
        .bind(s.plan_discount_monthly)
        .bind(ts(s.base.created_at))
        .bind(ts(s.base.updated_at))
        .bind(s.base.deleted_at.map(ts))
        .bind(s.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn update_subscription(&self, s: &Subscription) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE subscriptions
                SET plan_kind = ?1, next_charge_date = ?2, status = ?3,
                    payment_method_kind = ?4, payment_method_label = ?5,
                    payment_method_expiry = ?6, gateway = ?7,
                    gateway_subscription_id = ?8, card_status = ?9,
                    pix_auto_rec_id = ?10, pix_auto_status = ?11,
                    plan_id = ?12, plan_name = ?13, plan_amount = ?14,
                    plan_period_months = ?15, trial_days = ?16,
                    plan_discount_monthly = ?17,
                    updated_at = ?18, synced = ?19
              WHERE company_id = ?20 AND id = ?21 AND deleted_at IS NULL",
        )
        .bind(s.plan_kind.as_str())
        .bind(s.next_charge_date.map(date_str))
        .bind(s.status.as_str())
        .bind(&s.payment_method.kind)
        .bind(&s.payment_method.label)
        .bind(&s.payment_method.expiry)
        .bind(&s.gateway)
        .bind(&s.gateway_subscription_id)
        .bind(&s.card_status)
        .bind(&s.pix_auto_rec_id)
        .bind(&s.pix_auto_status)
        .bind(s.plan_id.map(|id| id.to_string()))
        .bind(&s.plan_name)
        .bind(s.plan_amount)
        .bind(s.plan_period_months as i64)
        .bind(s.trial_days as i64)
        .bind(s.plan_discount_monthly)
        .bind(ts(s.base.updated_at))
        .bind(s.base.synced)
        .bind(s.base.company_id.to_string())
        .bind(s.base.id.to_string())
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn find_invoices(&self, company_id: Uuid) -> Result<Vec<Invoice>, CoreError> {
        let rows = sqlx::query_as::<_, InvoiceRow>(
            "SELECT * FROM subscription_invoices
             WHERE company_id = ?1 AND deleted_at IS NULL
             ORDER BY issued_at DESC",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(Invoice::try_from).collect()
    }

    async fn create_invoice(&self, inv: &Invoice) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO subscription_invoices
             (id, company_id, subscription_id, number, description, amount,
              method_kind, method_label, status, issued_at, paid_at,
              created_at, updated_at, deleted_at, synced)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
        )
        .bind(inv.base.id.to_string())
        .bind(inv.base.company_id.to_string())
        .bind(inv.subscription_id.to_string())
        .bind(&inv.number)
        .bind(&inv.description)
        .bind(inv.amount)
        .bind(&inv.method_kind)
        .bind(&inv.method_label)
        .bind(inv.status.as_str())
        .bind(date_str(inv.issued_at))
        .bind(inv.paid_at.map(ts))
        .bind(ts(inv.base.created_at))
        .bind(ts(inv.base.updated_at))
        .bind(inv.base.deleted_at.map(ts))
        .bind(inv.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn update_invoice(&self, inv: &Invoice) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE subscription_invoices
                SET subscription_id = ?1, number = ?2, description = ?3, amount = ?4,
                    method_kind = ?5, method_label = ?6, status = ?7, issued_at = ?8,
                    paid_at = ?9, updated_at = ?10, synced = ?11
              WHERE company_id = ?12 AND id = ?13 AND deleted_at IS NULL",
        )
        .bind(inv.subscription_id.to_string())
        .bind(&inv.number)
        .bind(&inv.description)
        .bind(inv.amount)
        .bind(&inv.method_kind)
        .bind(&inv.method_label)
        .bind(inv.status.as_str())
        .bind(date_str(inv.issued_at))
        .bind(inv.paid_at.map(ts))
        .bind(ts(inv.base.updated_at))
        .bind(inv.base.synced)
        .bind(inv.base.company_id.to_string())
        .bind(inv.base.id.to_string())
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
               AND next_charge_date <= ?1",
        )
        .bind(date_str(today))
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(Subscription::try_from).collect()
    }

    async fn find_overdue_candidates(
        &self,
        today: chrono::NaiveDate,
        grace_days: i64,
    ) -> Result<Vec<Subscription>, CoreError> {
        // SQLite não tem `interval`; calculamos o cutoff em Rust.
        let cutoff = today - chrono::Duration::days(grace_days);
        let rows = sqlx::query_as::<_, SubscriptionRow>(
            "SELECT s.* FROM subscriptions s
             WHERE s.deleted_at IS NULL
               AND s.status = 'active'
               AND EXISTS (
                 SELECT 1 FROM subscription_invoices i
                 WHERE i.subscription_id = s.id
                   AND i.deleted_at IS NULL
                   AND i.status = 'pending'
                   AND i.issued_at < ?1
               )",
        )
        .bind(date_str(cutoff))
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(Subscription::try_from).collect()
    }

    async fn find_by_gateway_subscription_id(
        &self,
        gateway_subscription_id: &str,
    ) -> Result<Option<Subscription>, CoreError> {
        // No desktop não recebe webhooks; existe só para satisfazer a
        // trait. Ainda assim a query é correta e isolada por gateway id.
        let row = sqlx::query_as::<_, SubscriptionRow>(
            "SELECT * FROM subscriptions
             WHERE gateway_subscription_id = ?1 AND deleted_at IS NULL
             LIMIT 1",
        )
        .bind(gateway_subscription_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        row.map(Subscription::try_from).transpose()
    }

    async fn find_by_pix_auto_rec_id(
        &self,
        rec_id: &str,
    ) -> Result<Option<Subscription>, CoreError> {
        let row = sqlx::query_as::<_, SubscriptionRow>(
            "SELECT * FROM subscriptions
             WHERE pix_auto_rec_id = ?1 AND deleted_at IS NULL
             LIMIT 1",
        )
        .bind(rec_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        row.map(Subscription::try_from).transpose()
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
        let row = sqlx::query_as::<_, InvoiceRow>(
            "SELECT * FROM subscription_invoices
             WHERE subscription_id = ?1
               AND deleted_at IS NULL
               AND issued_at >= ?2
               AND issued_at < ?3
             ORDER BY issued_at DESC
             LIMIT 1",
        )
        .bind(subscription_id.to_string())
        .bind(date_str(start))
        .bind(date_str(end))
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        row.map(Invoice::try_from).transpose()
    }

    async fn find_unsynced_subscriptions(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<Subscription>, CoreError> {
        let rows = sqlx::query_as::<_, SubscriptionRow>(
            "SELECT * FROM subscriptions WHERE company_id = ?1 AND synced = 0",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(Subscription::try_from).collect()
    }

    async fn find_unsynced_invoices(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<Invoice>, CoreError> {
        let rows = sqlx::query_as::<_, InvoiceRow>(
            "SELECT * FROM subscription_invoices WHERE company_id = ?1 AND synced = 0",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(Invoice::try_from).collect()
    }

    async fn mark_subscription_synced(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<(), CoreError> {
        sqlx::query("UPDATE subscriptions SET synced = 1 WHERE company_id = ?1 AND id = ?2")
            .bind(company_id.to_string())
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(map_db)?;
        Ok(())
    }

    async fn mark_invoice_synced(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE subscription_invoices SET synced = 1 WHERE company_id = ?1 AND id = ?2",
        )
        .bind(company_id.to_string())
        .bind(id.to_string())
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
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23)
             ON CONFLICT (id) DO UPDATE SET
                plan_kind = excluded.plan_kind,
                next_charge_date = excluded.next_charge_date,
                status = excluded.status,
                payment_method_kind = excluded.payment_method_kind,
                payment_method_label = excluded.payment_method_label,
                payment_method_expiry = excluded.payment_method_expiry,
                gateway = excluded.gateway,
                gateway_subscription_id = excluded.gateway_subscription_id,
                card_status = excluded.card_status,
                pix_auto_rec_id = excluded.pix_auto_rec_id,
                pix_auto_status = excluded.pix_auto_status,
                plan_id = excluded.plan_id,
                plan_name = excluded.plan_name,
                plan_amount = excluded.plan_amount,
                plan_period_months = excluded.plan_period_months,
                trial_days = excluded.trial_days,
                plan_discount_monthly = excluded.plan_discount_monthly,
                updated_at = excluded.updated_at,
                deleted_at = excluded.deleted_at,
                synced = excluded.synced
             WHERE excluded.updated_at > subscriptions.updated_at",
        )
        .bind(s.base.id.to_string())
        .bind(s.base.company_id.to_string())
        .bind(s.plan_kind.as_str())
        .bind(s.next_charge_date.map(date_str))
        .bind(s.status.as_str())
        .bind(&s.payment_method.kind)
        .bind(&s.payment_method.label)
        .bind(&s.payment_method.expiry)
        .bind(&s.gateway)
        .bind(&s.gateway_subscription_id)
        .bind(&s.card_status)
        .bind(&s.pix_auto_rec_id)
        .bind(&s.pix_auto_status)
        .bind(s.plan_id.map(|id| id.to_string()))
        .bind(&s.plan_name)
        .bind(s.plan_amount)
        .bind(s.plan_period_months as i64)
        .bind(s.trial_days as i64)
        .bind(s.plan_discount_monthly)
        .bind(ts(s.base.created_at))
        .bind(ts(s.base.updated_at))
        .bind(s.base.deleted_at.map(ts))
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
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)
             ON CONFLICT (id) DO UPDATE SET
                subscription_id = excluded.subscription_id,
                number = excluded.number,
                description = excluded.description,
                amount = excluded.amount,
                method_kind = excluded.method_kind,
                method_label = excluded.method_label,
                status = excluded.status,
                issued_at = excluded.issued_at,
                paid_at = excluded.paid_at,
                updated_at = excluded.updated_at,
                deleted_at = excluded.deleted_at,
                synced = excluded.synced
             WHERE excluded.updated_at > subscription_invoices.updated_at",
        )
        .bind(inv.base.id.to_string())
        .bind(inv.base.company_id.to_string())
        .bind(inv.subscription_id.to_string())
        .bind(&inv.number)
        .bind(&inv.description)
        .bind(inv.amount)
        .bind(&inv.method_kind)
        .bind(&inv.method_label)
        .bind(inv.status.as_str())
        .bind(date_str(inv.issued_at))
        .bind(inv.paid_at.map(ts))
        .bind(ts(inv.base.created_at))
        .bind(ts(inv.base.updated_at))
        .bind(inv.base.deleted_at.map(ts))
        .bind(inv.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn find_subscriptions_updated_since(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
    ) -> Result<Vec<Subscription>, CoreError> {
        let rows = sqlx::query_as::<_, SubscriptionRow>(
            "SELECT * FROM subscriptions WHERE company_id = ?1 AND updated_at > ?2",
        )
        .bind(company_id.to_string())
        .bind(ts(since))
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(Subscription::try_from).collect()
    }

    async fn find_invoices_updated_since(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
    ) -> Result<Vec<Invoice>, CoreError> {
        let rows = sqlx::query_as::<_, InvoiceRow>(
            "SELECT * FROM subscription_invoices WHERE company_id = ?1 AND updated_at > ?2",
        )
        .bind(company_id.to_string())
        .bind(ts(since))
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(Invoice::try_from).collect()
    }
}
