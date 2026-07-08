use async_trait::async_trait;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::PgPool;
use uuid::Uuid;

use letaf_core::company::model::Company;
use letaf_core::company::repository::CompanyRepository;
use letaf_core::error::CoreError;

use super::helpers::map_db;

#[derive(FromRow)]
struct CompanyRow {
    id: Uuid,
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
    products_per_page: i32,
    orders_per_page: i32,
    utc_offset_minutes: i32,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    deleted_at: Option<NaiveDateTime>,
    synced: bool,
}

impl From<CompanyRow> for Company {
    fn from(r: CompanyRow) -> Self {
        Self {
            id: r.id,
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
            products_per_page: r.products_per_page,
            orders_per_page: r.orders_per_page,
            utc_offset_minutes: r.utc_offset_minutes,
            created_at: r.created_at,
            updated_at: r.updated_at,
            deleted_at: r.deleted_at,
            synced: r.synced,
        }
    }
}

/// Implementação PostgreSQL do CompanyRepository.
///
/// Regras aplicadas (AI_RULES.md §2, §5, §6, §10):
/// - Multi-tenant: mapear subdomínio → company_id
/// - Servidor usa PostgreSQL
/// - Soft delete via deleted_at
/// - Acesso ao banco somente via repository
pub struct PgCompanyRepository {
    pool: PgPool,
}

impl PgCompanyRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CompanyRepository for PgCompanyRepository {
    async fn find_by_id(&self, id: Uuid) -> Result<Option<Company>, CoreError> {
        sqlx::query_as::<_, CompanyRow>(
            "SELECT * FROM companies WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map(|opt| opt.map(Company::from))
        .map_err(map_db)
    }

    async fn find_by_subdomain(&self, subdomain: &str) -> Result<Option<Company>, CoreError> {
        sqlx::query_as::<_, CompanyRow>(
            "SELECT * FROM companies WHERE subdomain = $1 AND deleted_at IS NULL",
        )
        .bind(subdomain)
        .fetch_optional(&self.pool)
        .await
        .map(|opt| opt.map(Company::from))
        .map_err(map_db)
    }

    async fn find_id_by_subdomain(&self, subdomain: &str) -> Result<Option<Uuid>, CoreError> {
        // Caminho quente (todo request passa pelo tenant): só o id, sem os
        // blobs `logo_data`/`cover_data` do `SELECT *` (§13).
        let row: Option<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM companies WHERE subdomain = $1 AND deleted_at IS NULL",
        )
        .bind(subdomain)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(row.map(|(id,)| id))
    }

    async fn find_all(&self) -> Result<Vec<Company>, CoreError> {
        let rows = sqlx::query_as::<_, CompanyRow>(
            "SELECT * FROM companies WHERE deleted_at IS NULL ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(rows.into_iter().map(Company::from).collect())
    }

    async fn create(&self, company: &Company) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO companies (id, name, subdomain, store_override, products_per_page, created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(company.id)
        .bind(&company.name)
        .bind(&company.subdomain)
        .bind(&company.store_override)
        .bind(company.products_per_page)
        .bind(company.created_at)
        .bind(company.updated_at)
        .bind(company.deleted_at)
        .bind(company.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    async fn update(&self, company: &Company) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE companies SET name = $1, subdomain = $2, store_override = $3,
             address = $4, phone = $5, whatsapp = $6, email = $7, instagram = $8,
             document = $9, neighborhood = $10, zip_code = $11, city = $12, uf = $13,
             logo_data = $14, cover_data = $15,
             products_per_page = $16, orders_per_page = $17, utc_offset_minutes = $18,
             updated_at = $19, synced = $20
             WHERE id = $21 AND deleted_at IS NULL",
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
        .bind(company.products_per_page)
        .bind(company.orders_per_page)
        .bind(company.utc_offset_minutes)
        .bind(company.updated_at)
        .bind(company.synced)
        .bind(company.id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }

    async fn soft_delete(&self, id: Uuid) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        sqlx::query(
            "UPDATE companies SET deleted_at = $1, updated_at = $2, synced = false
             WHERE id = $3 AND deleted_at IS NULL",
        )
        .bind(now)
        .bind(now)
        .bind(id)
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

        Ok(rows.into_iter().map(Company::from).collect())
    }

    async fn mark_synced(&self, id: Uuid) -> Result<(), CoreError> {
        sqlx::query("UPDATE companies SET synced = true WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(map_db)?;

        Ok(())
    }

    async fn find_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<Company>, CoreError> {
        let rows = sqlx::query_as::<_, CompanyRow>(
            "SELECT * FROM companies WHERE id = $1 AND updated_at > $2",
        )
        .bind(company_id)
        .bind(since)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(rows.into_iter().map(Company::from).collect())
    }

    async fn sync_upsert(&self, company: &Company) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO companies (id, name, subdomain, store_override,
                address, phone, whatsapp, email, instagram, document,
                neighborhood, zip_code, city, uf,
                logo_data, cover_data, products_per_page, orders_per_page,
                utc_offset_minutes, created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12,
                $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23)
             ON CONFLICT (id) DO UPDATE SET
                 name = EXCLUDED.name,
                 subdomain = EXCLUDED.subdomain,
                 store_override = EXCLUDED.store_override,
                 address = EXCLUDED.address,
                 phone = EXCLUDED.phone,
                 whatsapp = EXCLUDED.whatsapp,
                 email = EXCLUDED.email,
                 instagram = EXCLUDED.instagram,
                 document = EXCLUDED.document,
                 neighborhood = EXCLUDED.neighborhood,
                 zip_code = EXCLUDED.zip_code,
                 city = EXCLUDED.city,
                 uf = EXCLUDED.uf,
                 logo_data = EXCLUDED.logo_data,
                 cover_data = EXCLUDED.cover_data,
                 products_per_page = EXCLUDED.products_per_page,
                 orders_per_page = EXCLUDED.orders_per_page,
                 utc_offset_minutes = EXCLUDED.utc_offset_minutes,
                 updated_at = EXCLUDED.updated_at,
                 deleted_at = EXCLUDED.deleted_at,
                 synced = EXCLUDED.synced
             WHERE EXCLUDED.updated_at > companies.updated_at",
        )
        .bind(company.id)
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
        .bind(company.products_per_page)
        .bind(company.orders_per_page)
        .bind(company.utc_offset_minutes)
        .bind(company.created_at)
        .bind(company.updated_at)
        .bind(company.deleted_at)
        .bind(company.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(())
    }
}
