use async_trait::async_trait;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::PgPool;
use uuid::Uuid;

use letaf_core::customer_address::model::CustomerAddress;
use letaf_core::customer_address::repository::CustomerAddressRepository;
use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;

use super::helpers::map_db;

/// Row intermediário para mapeamento sqlx → domínio.
///
/// Regras aplicadas (AI_RULES.md §1, §10):
/// - Core não depende de sqlx
/// - Row struct vive na camada server (infraestrutura)
#[derive(FromRow)]
struct CustomerAddressRow {
    id: Uuid,
    company_id: Uuid,
    customer_id: Uuid,
    label: String,
    custom_label: Option<String>,
    street: String,
    number: String,
    neighborhood: String,
    apartment: Option<String>,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    deleted_at: Option<NaiveDateTime>,
    synced: bool,
}

impl From<CustomerAddressRow> for CustomerAddress {
    fn from(r: CustomerAddressRow) -> Self {
        Self {
            base: BaseFields {
                id: r.id,
                company_id: r.company_id,
                created_at: r.created_at,
                updated_at: r.updated_at,
                deleted_at: r.deleted_at,
                synced: r.synced,
            },
            customer_id: r.customer_id,
            label: r.label,
            custom_label: r.custom_label,
            street: r.street,
            number: r.number,
            neighborhood: r.neighborhood,
            apartment: r.apartment,
        }
    }
}

/// Implementação PostgreSQL do CustomerAddressRepository.
pub struct PgCustomerAddressRepository {
    pool: PgPool,
}

impl PgCustomerAddressRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CustomerAddressRepository for PgCustomerAddressRepository {
    async fn find_by_customer(
        &self,
        company_id: Uuid,
        customer_id: Uuid,
    ) -> Result<Vec<CustomerAddress>, CoreError> {
        let rows = sqlx::query_as::<_, CustomerAddressRow>(
            "SELECT * FROM customer_addresses \
             WHERE company_id = $1 AND customer_id = $2 AND deleted_at IS NULL \
             ORDER BY created_at DESC",
        )
        .bind(company_id)
        .bind(customer_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(rows.into_iter().map(CustomerAddress::from).collect())
    }

    async fn find_by_company(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<CustomerAddress>, CoreError> {
        let rows = sqlx::query_as::<_, CustomerAddressRow>(
            "SELECT * FROM customer_addresses \
             WHERE company_id = $1 AND deleted_at IS NULL \
             ORDER BY created_at DESC",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(rows.into_iter().map(CustomerAddress::from).collect())
    }

    async fn find_by_id(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<Option<CustomerAddress>, CoreError> {
        sqlx::query_as::<_, CustomerAddressRow>(
            "SELECT * FROM customer_addresses \
             WHERE company_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(company_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map(|opt| opt.map(CustomerAddress::from))
        .map_err(map_db)
    }

    async fn create(&self, address: &CustomerAddress) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO customer_addresses \
             (id, company_id, customer_id, label, custom_label, street, number, \
              neighborhood, apartment, created_at, updated_at, deleted_at, synced) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)",
        )
        .bind(address.base.id)
        .bind(address.base.company_id)
        .bind(address.customer_id)
        .bind(&address.label)
        .bind(&address.custom_label)
        .bind(&address.street)
        .bind(&address.number)
        .bind(&address.neighborhood)
        .bind(&address.apartment)
        .bind(address.base.created_at)
        .bind(address.base.updated_at)
        .bind(address.base.deleted_at)
        .bind(address.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    async fn update(&self, address: &CustomerAddress) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE customer_addresses \
             SET label = $1, custom_label = $2, street = $3, number = $4, \
                 neighborhood = $5, apartment = $6, updated_at = $7, synced = false \
             WHERE company_id = $8 AND id = $9 AND customer_id = $10 \
               AND deleted_at IS NULL",
        )
        .bind(&address.label)
        .bind(&address.custom_label)
        .bind(&address.street)
        .bind(&address.number)
        .bind(&address.neighborhood)
        .bind(&address.apartment)
        .bind(address.base.updated_at)
        .bind(address.base.company_id)
        .bind(address.base.id)
        .bind(address.customer_id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn soft_delete(
        &self,
        company_id: Uuid,
        id: Uuid,
        customer_id: Uuid,
    ) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        sqlx::query(
            "UPDATE customer_addresses \
             SET deleted_at = $1, updated_at = $2, synced = false \
             WHERE company_id = $3 AND id = $4 AND customer_id = $5 \
               AND deleted_at IS NULL",
        )
        .bind(now)
        .bind(now)
        .bind(company_id)
        .bind(id)
        .bind(customer_id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<CustomerAddress>, CoreError> {
        let rows = sqlx::query_as::<_, CustomerAddressRow>(
            "SELECT * FROM customer_addresses WHERE company_id = $1 AND synced = false",
        )
        .bind(company_id)
        .fetch_all(&self.pool).await.map_err(map_db)?;
        Ok(rows.into_iter().map(CustomerAddress::from).collect())
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        sqlx::query("UPDATE customer_addresses SET synced = true WHERE company_id = $1 AND id = $2")
            .bind(company_id).bind(id)
            .execute(&self.pool).await.map_err(map_db)?;
        Ok(())
    }

    async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
    ) -> Result<Vec<CustomerAddress>, CoreError> {
        let rows = sqlx::query_as::<_, CustomerAddressRow>(
            "SELECT * FROM customer_addresses WHERE company_id = $1 AND updated_at > $2",
        )
        .bind(company_id).bind(since)
        .fetch_all(&self.pool).await.map_err(map_db)?;
        Ok(rows.into_iter().map(CustomerAddress::from).collect())
    }

    async fn sync_upsert(&self, a: &CustomerAddress) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO customer_addresses \
             (id, company_id, customer_id, label, custom_label, street, number, \
              neighborhood, apartment, created_at, updated_at, deleted_at, synced) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13) \
             ON CONFLICT (id) DO UPDATE SET \
                 label = EXCLUDED.label, \
                 custom_label = EXCLUDED.custom_label, \
                 street = EXCLUDED.street, \
                 number = EXCLUDED.number, \
                 neighborhood = EXCLUDED.neighborhood, \
                 apartment = EXCLUDED.apartment, \
                 updated_at = EXCLUDED.updated_at, \
                 deleted_at = EXCLUDED.deleted_at, \
                 synced = EXCLUDED.synced \
             WHERE EXCLUDED.updated_at > customer_addresses.updated_at AND customer_addresses.company_id = EXCLUDED.company_id",
        )
        .bind(a.base.id)
        .bind(a.base.company_id)
        .bind(a.customer_id)
        .bind(&a.label)
        .bind(&a.custom_label)
        .bind(&a.street)
        .bind(&a.number)
        .bind(&a.neighborhood)
        .bind(&a.apartment)
        .bind(a.base.created_at)
        .bind(a.base.updated_at)
        .bind(a.base.deleted_at)
        .bind(a.base.synced)
        .execute(&self.pool).await.map_err(map_db)?;
        Ok(())
    }
}
