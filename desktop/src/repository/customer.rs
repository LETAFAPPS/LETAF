use async_trait::async_trait;
use sqlx::prelude::FromRow;
use sqlx::SqlitePool;
use uuid::Uuid;

use letaf_core::error::CoreError;
use letaf_core::customer::model::Customer;
use letaf_core::customer::repository::CustomerRepository;

use super::helpers::{parse_base, map_db, ts};

#[derive(FromRow)]
struct CustomerRow {
    id: String,
    company_id: String,
    name: String,
    email: Option<String>,
    phone: Option<String>,
    document: Option<String>,
    password_hash: Option<String>,
    profile_picture: Option<String>,
    notes: Option<String>,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    synced: bool,
}

impl TryFrom<CustomerRow> for Customer {
    type Error = CoreError;

    fn try_from(r: CustomerRow) -> Result<Self, Self::Error> {
        Ok(Self {
            base: parse_base(&r.id, &r.company_id, &r.created_at, &r.updated_at, r.deleted_at.as_deref(), r.synced)?,
            name: r.name,
            email: r.email,
            phone: r.phone,
            document: r.document,
            password_hash: r.password_hash,
            profile_picture: r.profile_picture,
            notes: r.notes,
        })
    }
}

/// Implementação SQLite do CustomerRepository.
///
/// Regras aplicadas (AI_RULES.md §3, §5, §7, §10):
/// - Desktop usa SQLite
/// - Todas queries filtram por company_id (isolamento)
/// - Soft delete via deleted_at
/// - Acesso ao banco somente via repository
/// - Offline-first: toda escrita ocorre primeiro no SQLite
pub struct SqliteCustomerRepository {
    pool: SqlitePool,
}

impl SqliteCustomerRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CustomerRepository for SqliteCustomerRepository {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Customer>, CoreError> {
        let row = sqlx::query_as::<_, CustomerRow>(
            "SELECT * FROM customers WHERE company_id = ?1 AND id = ?2 AND deleted_at IS NULL",
        )
        .bind(company_id.to_string())
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;

        row.map(Customer::try_from).transpose()
    }

    async fn find_by_email(&self, company_id: Uuid, email: &str) -> Result<Option<Customer>, CoreError> {
        let row = sqlx::query_as::<_, CustomerRow>(
            "SELECT * FROM customers WHERE company_id = ?1 AND email = ?2 AND deleted_at IS NULL",
        )
        .bind(company_id.to_string())
        .bind(email)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;

        row.map(Customer::try_from).transpose()
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<Customer>, CoreError> {
        let rows = sqlx::query_as::<_, CustomerRow>(
            "SELECT * FROM customers WHERE company_id = ?1 AND deleted_at IS NULL ORDER BY created_at DESC",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        rows.into_iter().map(Customer::try_from).collect()
    }

    async fn create(&self, customer: &Customer) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO customers (id, company_id, name, email, phone, document, password_hash, profile_picture, created_at, updated_at, deleted_at, synced, notes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        )
        .bind(customer.base.id.to_string())
        .bind(customer.base.company_id.to_string())
        .bind(&customer.name)
        .bind(&customer.email)
        .bind(&customer.phone)
        .bind(&customer.document)
        .bind(&customer.password_hash)
        .bind(&customer.profile_picture)
        .bind(ts(customer.base.created_at))
        .bind(ts(customer.base.updated_at))
        .bind(customer.base.deleted_at.map(ts))
        .bind(customer.base.synced)
        .bind(&customer.notes)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    async fn update(&self, customer: &Customer) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE customers SET name = ?1, email = ?2, phone = ?3, document = ?4, password_hash = ?5, profile_picture = ?6, updated_at = ?7, synced = ?8, notes = ?9
             WHERE company_id = ?10 AND id = ?11 AND deleted_at IS NULL",
        )
        .bind(&customer.name)
        .bind(&customer.email)
        .bind(&customer.phone)
        .bind(&customer.document)
        .bind(&customer.password_hash)
        .bind(&customer.profile_picture)
        .bind(ts(customer.base.updated_at))
        .bind(customer.base.synced)
        .bind(&customer.notes)
        .bind(customer.base.company_id.to_string())
        .bind(customer.base.id.to_string())
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let now = ts(chrono::Utc::now().naive_utc());
        sqlx::query(
            "UPDATE customers SET deleted_at = ?1, updated_at = ?2, synced = false
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

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Customer>, CoreError> {
        let rows = sqlx::query_as::<_, CustomerRow>(
            "SELECT * FROM customers WHERE company_id = ?1 AND synced = false",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        rows.into_iter().map(Customer::try_from).collect()
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query("UPDATE customers SET synced = true WHERE company_id = ?1 AND id = ?2 AND updated_at = ?3")
            .bind(company_id.to_string())
            .bind(id.to_string())
            .bind(ts(updated_at))
            .execute(&self.pool)
            .await
            .map_err(map_db)?;

        Ok(())
    }

    async fn find_updated_since(&self, company_id: Uuid, since: chrono::NaiveDateTime) -> Result<Vec<Customer>, CoreError> {
        let rows = sqlx::query_as::<_, CustomerRow>(
            "SELECT * FROM customers WHERE company_id = ?1 AND updated_at > ?2",
        )
        .bind(company_id.to_string())
        .bind(ts(since))
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        rows.into_iter().map(Customer::try_from).collect()
    }

    async fn sync_upsert(&self, customer: &Customer) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO customers (id, company_id, name, email, phone, document, password_hash, profile_picture, created_at, updated_at, deleted_at, synced, notes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT (id) DO UPDATE SET
                 name = excluded.name,
                 email = excluded.email,
                 phone = excluded.phone,
                 document = excluded.document,
                 password_hash = excluded.password_hash,
                 profile_picture = excluded.profile_picture,
                 updated_at = excluded.updated_at,
                 deleted_at = excluded.deleted_at,
                 synced = excluded.synced,
                 notes = excluded.notes
             WHERE excluded.updated_at > customers.updated_at",
        )
        .bind(customer.base.id.to_string())
        .bind(customer.base.company_id.to_string())
        .bind(&customer.name)
        .bind(&customer.email)
        .bind(&customer.phone)
        .bind(&customer.document)
        .bind(&customer.password_hash)
        .bind(&customer.profile_picture)
        .bind(ts(customer.base.created_at))
        .bind(ts(customer.base.updated_at))
        .bind(customer.base.deleted_at.map(ts))
        .bind(customer.base.synced)
        .bind(&customer.notes)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }
}
