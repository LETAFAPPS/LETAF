use async_trait::async_trait;
use sqlx::prelude::FromRow;
use sqlx::SqlitePool;
use uuid::Uuid;

use letaf_core::company::model::Company;
use letaf_core::company::repository::CompanyRepository;
use letaf_core::error::CoreError;

use super::helpers::{map_db, parse_timestamp, parse_uuid, ts};

#[derive(FromRow)]
struct CompanyRow {
    id: String,
    name: String,
    subdomain: String,
    store_override: String,
    address: Option<String>,
    phone: Option<String>,
    whatsapp: Option<String>,
    email: Option<String>,
    instagram: Option<String>,
    document: Option<String>,
    neighborhood: Option<String>,
    zip_code: Option<String>,
    city: Option<String>,
    uf: Option<String>,
    logo_data: Option<String>,
    cover_data: Option<String>,
    products_per_page: i64,
    orders_per_page: i64,
    utc_offset_minutes: i64,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    synced: bool,
}

impl TryFrom<CompanyRow> for Company {
    type Error = CoreError;

    fn try_from(r: CompanyRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: parse_uuid(&r.id)?,
            name: r.name,
            subdomain: r.subdomain,
            store_override: r.store_override,
            address: r.address,
            phone: r.phone,
            whatsapp: r.whatsapp,
            email: r.email,
            instagram: r.instagram,
            document: r.document,
            neighborhood: r.neighborhood,
            zip_code: r.zip_code,
            city: r.city,
            uf: r.uf,
            logo_data: r.logo_data,
            cover_data: r.cover_data,
            products_per_page: r.products_per_page as i32,
            orders_per_page: r.orders_per_page as i32,
            utc_offset_minutes: r.utc_offset_minutes as i32,
            created_at: parse_timestamp(&r.created_at)?,
            updated_at: parse_timestamp(&r.updated_at)?,
            deleted_at: r.deleted_at.as_deref().map(parse_timestamp).transpose()?,
            synced: r.synced,
        })
    }
}

/// Implementação SQLite do CompanyRepository.
///
/// Regras aplicadas (AI_RULES.md §5, §6, §7, §10):
/// - Desktop usa SQLite
/// - Soft delete via deleted_at
/// - Offline-first
pub struct SqliteCompanyRepository {
    pool: SqlitePool,
}

impl SqliteCompanyRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CompanyRepository for SqliteCompanyRepository {
    async fn find_by_id(&self, id: Uuid) -> Result<Option<Company>, CoreError> {
        let row = sqlx::query_as::<_, CompanyRow>(
            "SELECT * FROM companies WHERE id = ?1 AND deleted_at IS NULL",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;

        row.map(Company::try_from).transpose()
    }

    async fn find_by_subdomain(&self, subdomain: &str) -> Result<Option<Company>, CoreError> {
        let row = sqlx::query_as::<_, CompanyRow>(
            "SELECT * FROM companies WHERE subdomain = ?1 AND deleted_at IS NULL",
        )
        .bind(subdomain)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;

        row.map(Company::try_from).transpose()
    }

    async fn find_all(&self) -> Result<Vec<Company>, CoreError> {
        let rows = sqlx::query_as::<_, CompanyRow>(
            "SELECT * FROM companies WHERE deleted_at IS NULL ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        rows.into_iter().map(Company::try_from).collect()
    }

    async fn create(&self, company: &Company) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO companies (id, name, subdomain, store_override, products_per_page, created_at, updated_at, deleted_at, synced)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )
        .bind(company.id.to_string())
        .bind(&company.name)
        .bind(&company.subdomain)
        .bind(&company.store_override)
        .bind(company.products_per_page as i64)
        .bind(ts(company.created_at))
        .bind(ts(company.updated_at))
        .bind(company.deleted_at.map(ts))
        .bind(company.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    async fn update(&self, company: &Company) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE companies SET name = ?1, subdomain = ?2, store_override = ?3,
             address = ?4, phone = ?5, whatsapp = ?6, email = ?7, instagram = ?8,
             document = ?9, neighborhood = ?10, zip_code = ?11, city = ?12, uf = ?13,
             logo_data = ?14, cover_data = ?15,
             products_per_page = ?16, orders_per_page = ?17, updated_at = ?18, synced = ?19
             WHERE id = ?20 AND deleted_at IS NULL",
        )
        .bind(&company.name)
        .bind(&company.subdomain)
        .bind(&company.store_override)
        .bind(&company.address)
        .bind(&company.phone)
        .bind(&company.whatsapp)
        .bind(&company.email)
        .bind(&company.instagram)
        .bind(&company.document)
        .bind(&company.neighborhood)
        .bind(&company.zip_code)
        .bind(&company.city)
        .bind(&company.uf)
        .bind(&company.logo_data)
        .bind(&company.cover_data)
        .bind(company.products_per_page as i64)
        .bind(company.orders_per_page as i64)
        .bind(ts(company.updated_at))
        .bind(company.synced)
        .bind(company.id.to_string())
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    async fn soft_delete(&self, id: Uuid) -> Result<(), CoreError> {
        let now = ts(chrono::Utc::now().naive_utc());
        sqlx::query(
            "UPDATE companies SET deleted_at = ?1, updated_at = ?2, synced = false
             WHERE id = ?3 AND deleted_at IS NULL",
        )
        .bind(&now)
        .bind(&now)
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    async fn find_unsynced(&self) -> Result<Vec<Company>, CoreError> {
        let rows = sqlx::query_as::<_, CompanyRow>(
            "SELECT * FROM companies WHERE synced = false",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        rows.into_iter().map(Company::try_from).collect()
    }

    async fn mark_synced(&self, id: Uuid) -> Result<(), CoreError> {
        sqlx::query("UPDATE companies SET synced = true WHERE id = ?1")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(map_db)?;

        Ok(())
    }

    async fn find_updated_since(&self, company_id: Uuid, since: chrono::NaiveDateTime) -> Result<Vec<Company>, CoreError> {
        let rows = sqlx::query_as::<_, CompanyRow>(
            "SELECT * FROM companies WHERE id = ?1 AND updated_at > ?2",
        )
        .bind(company_id.to_string())
        .bind(ts(since))
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        rows.into_iter().map(Company::try_from).collect()
    }

    async fn sync_upsert(&self, company: &Company) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO companies (id, name, subdomain, store_override,
                address, phone, whatsapp, email, instagram, document,
                neighborhood, zip_code, city, uf,
                logo_data, cover_data, products_per_page, orders_per_page,
                created_at, updated_at, deleted_at, synced)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)
             ON CONFLICT (id) DO UPDATE SET
                 name = excluded.name,
                 subdomain = excluded.subdomain,
                 store_override = excluded.store_override,
                 address = excluded.address,
                 phone = excluded.phone,
                 whatsapp = excluded.whatsapp,
                 email = excluded.email,
                 instagram = excluded.instagram,
                 document = excluded.document,
                 neighborhood = excluded.neighborhood,
                 zip_code = excluded.zip_code,
                 city = excluded.city,
                 uf = excluded.uf,
                 logo_data = excluded.logo_data,
                 cover_data = excluded.cover_data,
                 products_per_page = excluded.products_per_page,
                 orders_per_page = excluded.orders_per_page,
                 updated_at = excluded.updated_at,
                 deleted_at = excluded.deleted_at,
                 synced = excluded.synced
             WHERE excluded.updated_at > companies.updated_at",
        )
        .bind(company.id.to_string())
        .bind(&company.name)
        .bind(&company.subdomain)
        .bind(&company.store_override)
        .bind(&company.address)
        .bind(&company.phone)
        .bind(&company.whatsapp)
        .bind(&company.email)
        .bind(&company.instagram)
        .bind(&company.document)
        .bind(&company.neighborhood)
        .bind(&company.zip_code)
        .bind(&company.city)
        .bind(&company.uf)
        .bind(&company.logo_data)
        .bind(&company.cover_data)
        .bind(company.products_per_page as i64)
        .bind(company.orders_per_page as i64)
        .bind(ts(company.created_at))
        .bind(ts(company.updated_at))
        .bind(company.deleted_at.map(ts))
        .bind(company.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }
}
