use async_trait::async_trait;
use sqlx::prelude::FromRow;
use sqlx::SqlitePool;
use uuid::Uuid;

use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;
use letaf_core::payment_method::model::PaymentMethod;
use letaf_core::payment_method::repository::PaymentMethodRepository;

use super::helpers::{map_db, parse_timestamp, parse_uuid, ts};

#[derive(FromRow)]
struct PaymentMethodRow {
    id: String,
    company_id: String,
    kind: String,
    label: String,
    masked: String,
    expiry: String,
    is_default: bool,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    synced: bool,
}

impl TryFrom<PaymentMethodRow> for PaymentMethod {
    type Error = CoreError;

    fn try_from(r: PaymentMethodRow) -> Result<Self, Self::Error> {
        Ok(Self {
            base: BaseFields {
                id: parse_uuid(&r.id)?,
                company_id: parse_uuid(&r.company_id)?,
                created_at: parse_timestamp(&r.created_at)?,
                updated_at: parse_timestamp(&r.updated_at)?,
                deleted_at: r.deleted_at.as_deref().map(parse_timestamp).transpose()?,
                synced: r.synced,
            },
            kind: r.kind,
            label: r.label,
            masked: r.masked,
            expiry: r.expiry,
            is_default: r.is_default,
        })
    }
}

pub struct SqlitePaymentMethodRepository {
    pool: SqlitePool,
}

impl SqlitePaymentMethodRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl PaymentMethodRepository for SqlitePaymentMethodRepository {
    async fn find_by_id(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<Option<PaymentMethod>, CoreError> {
        let row = sqlx::query_as::<_, PaymentMethodRow>(
            "SELECT * FROM payment_methods
             WHERE company_id = ?1 AND id = ?2 AND deleted_at IS NULL",
        )
        .bind(company_id.to_string())
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        row.map(PaymentMethod::try_from).transpose()
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<PaymentMethod>, CoreError> {
        let rows = sqlx::query_as::<_, PaymentMethodRow>(
            "SELECT * FROM payment_methods
             WHERE company_id = ?1 AND deleted_at IS NULL
             ORDER BY is_default DESC, created_at ASC",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(PaymentMethod::try_from).collect()
    }

    async fn find_default(&self, company_id: Uuid) -> Result<Option<PaymentMethod>, CoreError> {
        let row = sqlx::query_as::<_, PaymentMethodRow>(
            "SELECT * FROM payment_methods
             WHERE company_id = ?1 AND is_default = 1 AND deleted_at IS NULL
             LIMIT 1",
        )
        .bind(company_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        row.map(PaymentMethod::try_from).transpose()
    }

    async fn create(&self, m: &PaymentMethod) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO payment_methods
             (id, company_id, kind, label, masked, expiry, is_default,
              created_at, updated_at, deleted_at, synced)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        )
        .bind(m.base.id.to_string())
        .bind(m.base.company_id.to_string())
        .bind(&m.kind)
        .bind(&m.label)
        .bind(&m.masked)
        .bind(&m.expiry)
        .bind(m.is_default)
        .bind(ts(m.base.created_at))
        .bind(ts(m.base.updated_at))
        .bind(m.base.deleted_at.map(ts))
        .bind(m.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn update(&self, m: &PaymentMethod) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE payment_methods
                SET kind = ?1, label = ?2, masked = ?3, expiry = ?4,
                    is_default = ?5, updated_at = ?6, synced = ?7
              WHERE company_id = ?8 AND id = ?9 AND deleted_at IS NULL",
        )
        .bind(&m.kind)
        .bind(&m.label)
        .bind(&m.masked)
        .bind(&m.expiry)
        .bind(m.is_default)
        .bind(ts(m.base.updated_at))
        .bind(m.base.synced)
        .bind(m.base.company_id.to_string())
        .bind(m.base.id.to_string())
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let now = ts(chrono::Utc::now().naive_utc());
        sqlx::query(
            "UPDATE payment_methods
                SET deleted_at = ?1, updated_at = ?2, synced = 0
              WHERE company_id = ?3 AND id = ?4 AND deleted_at IS NULL",
        )
        .bind(&now)
        .bind(&now)
        .bind(company_id.to_string())
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn clear_default(&self, company_id: Uuid) -> Result<(), CoreError> {
        let now = ts(chrono::Utc::now().naive_utc());
        sqlx::query(
            "UPDATE payment_methods
                SET is_default = 0, updated_at = ?1, synced = 0
              WHERE company_id = ?2 AND is_default = 1 AND deleted_at IS NULL",
        )
        .bind(&now)
        .bind(company_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<PaymentMethod>, CoreError> {
        let rows = sqlx::query_as::<_, PaymentMethodRow>(
            "SELECT * FROM payment_methods WHERE company_id = ?1 AND synced = 0",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(PaymentMethod::try_from).collect()
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE payment_methods SET synced = 1 WHERE company_id = ?1 AND id = ?2",
        )
        .bind(company_id.to_string())
        .bind(id.to_string())
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
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
             ON CONFLICT (id) DO UPDATE SET
                kind = excluded.kind,
                label = excluded.label,
                masked = excluded.masked,
                expiry = excluded.expiry,
                is_default = excluded.is_default,
                updated_at = excluded.updated_at,
                deleted_at = excluded.deleted_at,
                synced = excluded.synced
             WHERE excluded.updated_at > payment_methods.updated_at",
        )
        .bind(m.base.id.to_string())
        .bind(m.base.company_id.to_string())
        .bind(&m.kind)
        .bind(&m.label)
        .bind(&m.masked)
        .bind(&m.expiry)
        .bind(m.is_default)
        .bind(ts(m.base.created_at))
        .bind(ts(m.base.updated_at))
        .bind(m.base.deleted_at.map(ts))
        .bind(m.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
    ) -> Result<Vec<PaymentMethod>, CoreError> {
        let rows = sqlx::query_as::<_, PaymentMethodRow>(
            "SELECT * FROM payment_methods WHERE company_id = ?1 AND updated_at > ?2",
        )
        .bind(company_id.to_string())
        .bind(ts(since))
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(PaymentMethod::try_from).collect()
    }
}
