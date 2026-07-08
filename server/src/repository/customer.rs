use async_trait::async_trait;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::PgPool;
use uuid::Uuid;

use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;
use letaf_core::customer::model::Customer;
use letaf_core::customer::repository::CustomerRepository;

use super::helpers::{keyset_pull_sql, map_db};

/// Row intermediário para mapeamento sqlx → domínio.
///
/// Regras aplicadas (AI_RULES.md §1, §10):
/// - Core não depende de sqlx
/// - Row struct vive na camada server (infraestrutura)
#[derive(FromRow)]
struct CustomerRow {
    id: Uuid,
    company_id: Uuid,
    name: String,
    email: Option<String>,
    phone: Option<String>,
    document: Option<String>,
    password_hash: Option<String>,
    profile_picture: Option<String>,
    notes: Option<String>,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    deleted_at: Option<NaiveDateTime>,
    synced: bool,
}

impl From<CustomerRow> for Customer {
    fn from(r: CustomerRow) -> Self {
        Self {
            base: BaseFields {
                id: r.id,
                company_id: r.company_id,
                created_at: r.created_at,
                updated_at: r.updated_at,
                deleted_at: r.deleted_at,
                synced: r.synced,
            },
            name: r.name,
            email: r.email,
            phone: r.phone,
            document: r.document,
            password_hash: r.password_hash,
            profile_picture: r.profile_picture,
            notes: r.notes,
        }
    }
}

/// Implementação PostgreSQL do CustomerRepository.
///
/// Regras aplicadas (AI_RULES.md §3, §5, §6, §10):
/// - Todas queries filtram por company_id (isolamento)
/// - Soft delete via deleted_at
/// - Servidor usa PostgreSQL
/// - Acesso ao banco somente via repository
pub struct PgCustomerRepository {
    pool: PgPool,
}

impl PgCustomerRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CustomerRepository for PgCustomerRepository {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Customer>, CoreError> {
        sqlx::query_as::<_, CustomerRow>(
            "SELECT * FROM customers WHERE company_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(company_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map(|opt| opt.map(Customer::from))
        .map_err(map_db)
    }

    async fn find_by_email(&self, company_id: Uuid, email: &str) -> Result<Option<Customer>, CoreError> {
        sqlx::query_as::<_, CustomerRow>(
            "SELECT * FROM customers WHERE company_id = $1 AND email = $2 AND deleted_at IS NULL",
        )
        .bind(company_id)
        .bind(email)
        .fetch_optional(&self.pool)
        .await
        .map(|opt| opt.map(Customer::from))
        .map_err(map_db)
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<Customer>, CoreError> {
        let rows = sqlx::query_as::<_, CustomerRow>(
            "SELECT * FROM customers WHERE company_id = $1 AND deleted_at IS NULL ORDER BY created_at DESC",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(rows.into_iter().map(Customer::from).collect())
    }

    async fn create(&self, customer: &Customer) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO customers (id, company_id, name, email, phone, document, password_hash, profile_picture, created_at, updated_at, deleted_at, synced, notes)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
        )
        .bind(customer.base.id)
        .bind(customer.base.company_id)
        .bind(&customer.name)
        .bind(&customer.email)
        .bind(&customer.phone)
        .bind(&customer.document)
        .bind(&customer.password_hash)
        .bind(&customer.profile_picture)
        .bind(customer.base.created_at)
        .bind(customer.base.updated_at)
        .bind(customer.base.deleted_at)
        .bind(customer.base.synced)
        .bind(&customer.notes)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    async fn update(&self, customer: &Customer) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE customers SET name = $1, email = $2, phone = $3, document = $4, password_hash = $5, profile_picture = $6, updated_at = $7, synced = $8, notes = $9
             WHERE company_id = $10 AND id = $11 AND deleted_at IS NULL",
        )
        .bind(&customer.name)
        .bind(&customer.email)
        .bind(&customer.phone)
        .bind(&customer.document)
        .bind(&customer.password_hash)
        .bind(&customer.profile_picture)
        .bind(customer.base.updated_at)
        .bind(customer.base.synced)
        .bind(&customer.notes)
        .bind(customer.base.company_id)
        .bind(customer.base.id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        sqlx::query(
            "UPDATE customers SET deleted_at = $1, updated_at = $2, synced = false
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

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Customer>, CoreError> {
        let rows = sqlx::query_as::<_, CustomerRow>(
            "SELECT * FROM customers WHERE company_id = $1 AND synced = false",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(rows.into_iter().map(Customer::from).collect())
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE customers SET synced = true WHERE company_id = $1 AND id = $2 AND updated_at = $3",
        )
        .bind(company_id)
        .bind(id)
        .bind(updated_at)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    async fn find_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<Customer>, CoreError> {
        let rows = sqlx::query_as::<_, CustomerRow>(
            "SELECT * FROM customers WHERE company_id = $1 AND updated_at > $2",
        )
        .bind(company_id)
        .bind(since)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(rows.into_iter().map(Customer::from).collect())
    }

    async fn find_updated_since_paged(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
        after_id: Uuid,
        limit: i64,
    ) -> Result<Vec<Customer>, CoreError> {
        let rows = sqlx::query_as::<_, CustomerRow>(&keyset_pull_sql("customers"))
        .bind(company_id)
        .bind(since)
        .bind(after_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(Customer::from).collect())
    }

    async fn sync_upsert(&self, customer: &Customer) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO customers (id, company_id, name, email, phone, document, password_hash, profile_picture, created_at, updated_at, deleted_at, synced, notes)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
             ON CONFLICT (id) DO UPDATE SET
                 name = EXCLUDED.name,
                 email = EXCLUDED.email,
                 phone = EXCLUDED.phone,
                 document = EXCLUDED.document,
                 password_hash = EXCLUDED.password_hash,
                 profile_picture = EXCLUDED.profile_picture,
                 updated_at = EXCLUDED.updated_at,
                 deleted_at = EXCLUDED.deleted_at,
                 synced = EXCLUDED.synced,
                 notes = EXCLUDED.notes
             WHERE EXCLUDED.updated_at > customers.updated_at AND customers.company_id = EXCLUDED.company_id",
        )
        .bind(customer.base.id)
        .bind(customer.base.company_id)
        .bind(&customer.name)
        .bind(&customer.email)
        .bind(&customer.phone)
        .bind(&customer.document)
        .bind(&customer.password_hash)
        .bind(&customer.profile_picture)
        .bind(customer.base.created_at)
        .bind(customer.base.updated_at)
        .bind(customer.base.deleted_at)
        .bind(customer.base.synced)
        .bind(&customer.notes)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }
}
