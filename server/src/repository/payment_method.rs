use async_trait::async_trait;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::PgPool;
use uuid::Uuid;

use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;
use letaf_core::payment_method::model::PaymentMethod;
use letaf_core::payment_method::repository::PaymentMethodRepository;

use super::helpers::map_db;

#[derive(FromRow)]
struct PaymentMethodRow {
    id: Uuid,
    company_id: Uuid,
    kind: String,
    label: String,
    masked: String,
    expiry: String,
    is_default: bool,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    deleted_at: Option<NaiveDateTime>,
    synced: bool,
}

impl From<PaymentMethodRow> for PaymentMethod {
    fn from(r: PaymentMethodRow) -> Self {
        Self {
            base: BaseFields {
                id: r.id,
                company_id: r.company_id,
                created_at: r.created_at,
                updated_at: r.updated_at,
                deleted_at: r.deleted_at,
                synced: r.synced,
            },
            kind: r.kind,
            label: r.label,
            masked: r.masked,
            expiry: r.expiry,
            is_default: r.is_default,
        }
    }
}

pub struct PgPaymentMethodRepository {
    pool: PgPool,
}

impl PgPaymentMethodRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl PaymentMethodRepository for PgPaymentMethodRepository {
    async fn find_by_id(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<Option<PaymentMethod>, CoreError> {
        sqlx::query_as::<_, PaymentMethodRow>(
            "SELECT * FROM payment_methods
             WHERE company_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(company_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map(|opt| opt.map(PaymentMethod::from))
        .map_err(map_db)
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<PaymentMethod>, CoreError> {
        let rows = sqlx::query_as::<_, PaymentMethodRow>(
            "SELECT * FROM payment_methods
             WHERE company_id = $1 AND deleted_at IS NULL
             ORDER BY is_default DESC, created_at ASC",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(PaymentMethod::from).collect())
    }

    async fn find_default(&self, company_id: Uuid) -> Result<Option<PaymentMethod>, CoreError> {
        sqlx::query_as::<_, PaymentMethodRow>(
            "SELECT * FROM payment_methods
             WHERE company_id = $1 AND is_default = TRUE AND deleted_at IS NULL
             LIMIT 1",
        )
        .bind(company_id)
        .fetch_optional(&self.pool)
        .await
        .map(|opt| opt.map(PaymentMethod::from))
        .map_err(map_db)
    }

    async fn create(&self, m: &PaymentMethod) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO payment_methods
             (id, company_id, kind, label, masked, expiry, is_default,
              created_at, updated_at, deleted_at, synced)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
        )
        .bind(m.base.id)
        .bind(m.base.company_id)
        .bind(&m.kind)
        .bind(&m.label)
        .bind(&m.masked)
        .bind(&m.expiry)
        .bind(m.is_default)
        .bind(m.base.created_at)
        .bind(m.base.updated_at)
        .bind(m.base.deleted_at)
        .bind(m.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn update(&self, m: &PaymentMethod) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE payment_methods
                SET kind = $1, label = $2, masked = $3, expiry = $4,
                    is_default = $5, updated_at = $6, synced = $7
              WHERE company_id = $8 AND id = $9 AND deleted_at IS NULL",
        )
        .bind(&m.kind)
        .bind(&m.label)
        .bind(&m.masked)
        .bind(&m.expiry)
        .bind(m.is_default)
        .bind(m.base.updated_at)
        .bind(m.base.synced)
        .bind(m.base.company_id)
        .bind(m.base.id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        sqlx::query(
            "UPDATE payment_methods
                SET deleted_at = $1, updated_at = $2, synced = FALSE
              WHERE company_id = $3 AND id = $4 AND deleted_at IS NULL",
        )
        .bind(now)
        .bind(now)
        .bind(company_id)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn clear_default(&self, company_id: Uuid) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        sqlx::query(
            "UPDATE payment_methods
                SET is_default = FALSE, updated_at = $1, synced = FALSE
              WHERE company_id = $2 AND is_default = TRUE AND deleted_at IS NULL",
        )
        .bind(now)
        .bind(company_id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<PaymentMethod>, CoreError> {
        let rows = sqlx::query_as::<_, PaymentMethodRow>(
            "SELECT * FROM payment_methods WHERE company_id = $1 AND synced = FALSE",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(PaymentMethod::from).collect())
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE payment_methods SET synced = TRUE WHERE company_id = $1 AND id = $2 AND updated_at = $3",
        )
        .bind(company_id)
        .bind(id)
        .bind(updated_at)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn sync_upsert(&self, m: &PaymentMethod) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO payment_methods
             (id, company_id, kind, label, masked, expiry, is_default,
              created_at, updated_at, deleted_at, synced)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
             ON CONFLICT (id) DO UPDATE SET
                kind = EXCLUDED.kind,
                label = EXCLUDED.label,
                masked = EXCLUDED.masked,
                expiry = EXCLUDED.expiry,
                is_default = EXCLUDED.is_default,
                updated_at = EXCLUDED.updated_at,
                deleted_at = EXCLUDED.deleted_at,
                synced = EXCLUDED.synced
             WHERE EXCLUDED.updated_at > payment_methods.updated_at AND payment_methods.company_id = EXCLUDED.company_id",
        )
        .bind(m.base.id)
        .bind(m.base.company_id)
        .bind(&m.kind)
        .bind(&m.label)
        .bind(&m.masked)
        .bind(&m.expiry)
        .bind(m.is_default)
        .bind(m.base.created_at)
        .bind(m.base.updated_at)
        .bind(m.base.deleted_at)
        .bind(m.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
    ) -> Result<Vec<PaymentMethod>, CoreError> {
        let rows = sqlx::query_as::<_, PaymentMethodRow>(
            "SELECT * FROM payment_methods WHERE company_id = $1 AND updated_at > $2",
        )
        .bind(company_id)
        .bind(since)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(PaymentMethod::from).collect())
    }
}
