use async_trait::async_trait;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::SqlitePool;
use uuid::Uuid;

use letaf_core::customer_address::model::CustomerAddress;
use letaf_core::customer_address::repository::CustomerAddressRepository;
use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;

use super::helpers::{map_db, parse_timestamp, parse_uuid, ts};

#[derive(FromRow)]
struct CustomerAddressRow {
    id: String,
    company_id: String,
    customer_id: String,
    label: String,
    custom_label: Option<String>,
    street: String,
    number: String,
    neighborhood: String,
    apartment: Option<String>,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    synced: bool,
}

impl TryFrom<CustomerAddressRow> for CustomerAddress {
    type Error = CoreError;
    fn try_from(r: CustomerAddressRow) -> Result<Self, Self::Error> {
        Ok(Self {
            base: BaseFields {
                id: parse_uuid(&r.id)?,
                company_id: parse_uuid(&r.company_id)?,
                created_at: parse_timestamp(&r.created_at)?,
                updated_at: parse_timestamp(&r.updated_at)?,
                deleted_at: r.deleted_at.as_deref().map(parse_timestamp).transpose()?,
                synced: r.synced,
            },
            customer_id: parse_uuid(&r.customer_id)?,
            label: r.label,
            custom_label: r.custom_label,
            street: r.street,
            number: r.number,
            neighborhood: r.neighborhood,
            apartment: r.apartment,
        })
    }
}

pub struct SqliteCustomerAddressRepository {
    pool: SqlitePool,
}

impl SqliteCustomerAddressRepository {
    pub fn new(pool: SqlitePool) -> Self { Self { pool } }
}

#[async_trait]
impl CustomerAddressRepository for SqliteCustomerAddressRepository {
    async fn find_by_customer(
        &self,
        company_id: Uuid,
        customer_id: Uuid,
    ) -> Result<Vec<CustomerAddress>, CoreError> {
        let rows = sqlx::query_as::<_, CustomerAddressRow>(
            "SELECT * FROM customer_addresses WHERE company_id = ?1 AND customer_id = ?2 AND deleted_at IS NULL ORDER BY created_at DESC",
        )
        .bind(company_id.to_string())
        .bind(customer_id.to_string())
        .fetch_all(&self.pool).await.map_err(map_db)?;
        rows.into_iter().map(CustomerAddress::try_from).collect()
    }

    async fn find_by_company(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<CustomerAddress>, CoreError> {
        let rows = sqlx::query_as::<_, CustomerAddressRow>(
            "SELECT * FROM customer_addresses WHERE company_id = ?1 AND deleted_at IS NULL ORDER BY created_at DESC",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool).await.map_err(map_db)?;
        rows.into_iter().map(CustomerAddress::try_from).collect()
    }

    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<CustomerAddress>, CoreError> {
        let row = sqlx::query_as::<_, CustomerAddressRow>(
            "SELECT * FROM customer_addresses WHERE company_id = ?1 AND id = ?2 AND deleted_at IS NULL",
        )
        .bind(company_id.to_string())
        .bind(id.to_string())
        .fetch_optional(&self.pool).await.map_err(map_db)?;
        row.map(CustomerAddress::try_from).transpose()
    }

    async fn create(&self, a: &CustomerAddress) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO customer_addresses (id, company_id, customer_id, label, custom_label, street, number, neighborhood, apartment, created_at, updated_at, deleted_at, synced)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
        )
        .bind(a.base.id.to_string())
        .bind(a.base.company_id.to_string())
        .bind(a.customer_id.to_string())
        .bind(&a.label)
        .bind(&a.custom_label)
        .bind(&a.street)
        .bind(&a.number)
        .bind(&a.neighborhood)
        .bind(&a.apartment)
        .bind(ts(a.base.created_at))
        .bind(ts(a.base.updated_at))
        .bind(a.base.deleted_at.map(ts))
        .bind(a.base.synced)
        .execute(&self.pool).await.map_err(map_db)?;
        Ok(())
    }

    async fn update(&self, a: &CustomerAddress) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE customer_addresses SET label = ?1, custom_label = ?2, street = ?3, number = ?4, neighborhood = ?5, apartment = ?6, updated_at = ?7, synced = 0
             WHERE company_id = ?8 AND id = ?9 AND customer_id = ?10 AND deleted_at IS NULL",
        )
        .bind(&a.label)
        .bind(&a.custom_label)
        .bind(&a.street)
        .bind(&a.number)
        .bind(&a.neighborhood)
        .bind(&a.apartment)
        .bind(ts(a.base.updated_at))
        .bind(a.base.company_id.to_string())
        .bind(a.base.id.to_string())
        .bind(a.customer_id.to_string())
        .execute(&self.pool).await.map_err(map_db)?;
        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid, customer_id: Uuid) -> Result<(), CoreError> {
        let now = ts(chrono::Utc::now().naive_utc());
        sqlx::query(
            "UPDATE customer_addresses SET deleted_at = ?1, updated_at = ?2, synced = 0
             WHERE company_id = ?3 AND id = ?4 AND customer_id = ?5 AND deleted_at IS NULL",
        )
        .bind(&now).bind(&now)
        .bind(company_id.to_string())
        .bind(id.to_string())
        .bind(customer_id.to_string())
        .execute(&self.pool).await.map_err(map_db)?;
        Ok(())
    }

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<CustomerAddress>, CoreError> {
        let rows = sqlx::query_as::<_, CustomerAddressRow>(
            "SELECT * FROM customer_addresses WHERE company_id = ?1 AND synced = 0",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool).await.map_err(map_db)?;
        rows.into_iter().map(CustomerAddress::try_from).collect()
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query("UPDATE customer_addresses SET synced = 1 WHERE company_id = ?1 AND id = ?2 AND updated_at = ?3")
            .bind(company_id.to_string()).bind(id.to_string())
            .bind(ts(updated_at))
            .execute(&self.pool).await.map_err(map_db)?;
        Ok(())
    }

    async fn find_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<CustomerAddress>, CoreError> {
        let rows = sqlx::query_as::<_, CustomerAddressRow>(
            "SELECT * FROM customer_addresses WHERE company_id = ?1 AND updated_at > ?2",
        )
        .bind(company_id.to_string())
        .bind(ts(since))
        .fetch_all(&self.pool).await.map_err(map_db)?;
        rows.into_iter().map(CustomerAddress::try_from).collect()
    }

    async fn sync_upsert(&self, a: &CustomerAddress) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO customer_addresses (id, company_id, customer_id, label, custom_label, street, number, neighborhood, apartment, created_at, updated_at, deleted_at, synced)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
             ON CONFLICT (id) DO UPDATE SET
                 label = excluded.label,
                 custom_label = excluded.custom_label,
                 street = excluded.street,
                 number = excluded.number,
                 neighborhood = excluded.neighborhood,
                 apartment = excluded.apartment,
                 updated_at = excluded.updated_at,
                 deleted_at = excluded.deleted_at,
                 synced = excluded.synced
             WHERE excluded.updated_at > customer_addresses.updated_at",
        )
        .bind(a.base.id.to_string())
        .bind(a.base.company_id.to_string())
        .bind(a.customer_id.to_string())
        .bind(&a.label)
        .bind(&a.custom_label)
        .bind(&a.street)
        .bind(&a.number)
        .bind(&a.neighborhood)
        .bind(&a.apartment)
        .bind(ts(a.base.created_at))
        .bind(ts(a.base.updated_at))
        .bind(a.base.deleted_at.map(ts))
        .bind(a.base.synced)
        .execute(&self.pool).await.map_err(map_db)?;
        Ok(())
    }
}
