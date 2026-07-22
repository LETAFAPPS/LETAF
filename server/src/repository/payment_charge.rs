use async_trait::async_trait;
use rust_decimal::Decimal;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::PgPool;
use uuid::Uuid;

use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;
use letaf_core::payment_gateway::model::{ChargeStatus, PaymentCharge};
use letaf_core::payment_gateway::repository::PaymentChargeRepository;

use super::helpers::map_db;

#[derive(FromRow)]
struct PaymentChargeRow {
    id: Uuid,
    company_id: Uuid,
    invoice_id: Option<Uuid>,
    gateway: String,
    method: String,
    txid: Option<String>,
    amount: Decimal,
    status: String,
    pix_copia_cola: Option<String>,
    qr_code_b64: Option<String>,
    expires_at: Option<NaiveDateTime>,
    paid_at: Option<NaiveDateTime>,
    last_error: Option<String>,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    deleted_at: Option<NaiveDateTime>,
    synced: bool,
}

impl From<PaymentChargeRow> for PaymentCharge {
    fn from(r: PaymentChargeRow) -> Self {
        Self {
            base: BaseFields {
                id: r.id,
                company_id: r.company_id,
                created_at: r.created_at,
                updated_at: r.updated_at,
                deleted_at: r.deleted_at,
                synced: r.synced,
            },
            invoice_id: r.invoice_id,
            gateway: r.gateway,
            method: r.method,
            txid: r.txid,
            amount: r.amount,
            status: ChargeStatus::from_str(&r.status),
            pix_copia_cola: r.pix_copia_cola,
            qr_code_b64: r.qr_code_b64,
            expires_at: r.expires_at,
            paid_at: r.paid_at,
            last_error: r.last_error,
        }
    }
}

pub struct PgPaymentChargeRepository {
    pool: PgPool,
}

impl PgPaymentChargeRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl PaymentChargeRepository for PgPaymentChargeRepository {
    async fn find_by_id(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<Option<PaymentCharge>, CoreError> {
        sqlx::query_as::<_, PaymentChargeRow>(
            "SELECT * FROM payment_charges
             WHERE company_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(company_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map(|opt| opt.map(PaymentCharge::from))
        .map_err(map_db)
    }

    async fn create(&self, c: &PaymentCharge) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO payment_charges
             (id, company_id, invoice_id, gateway, method, txid, amount, status,
              pix_copia_cola, qr_code_b64, expires_at, paid_at, last_error,
              created_at, updated_at, deleted_at, synced)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)",
        )
        .bind(c.base.id)
        .bind(c.base.company_id)
        .bind(c.invoice_id)
        .bind(&c.gateway)
        .bind(&c.method)
        .bind(&c.txid)
        .bind(c.amount)
        .bind(c.status.as_str())
        .bind(&c.pix_copia_cola)
        .bind(&c.qr_code_b64)
        .bind(c.expires_at)
        .bind(c.paid_at)
        .bind(&c.last_error)
        .bind(c.base.created_at)
        .bind(c.base.updated_at)
        .bind(c.base.deleted_at)
        .bind(c.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn update(&self, c: &PaymentCharge) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE payment_charges
                SET invoice_id = $1, gateway = $2, method = $3, txid = $4,
                    amount = $5, status = $6, pix_copia_cola = $7, qr_code_b64 = $8,
                    expires_at = $9, paid_at = $10, last_error = $11,
                    updated_at = $12, synced = $13
              WHERE company_id = $14 AND id = $15 AND deleted_at IS NULL",
        )
        .bind(c.invoice_id)
        .bind(&c.gateway)
        .bind(&c.method)
        .bind(&c.txid)
        .bind(c.amount)
        .bind(c.status.as_str())
        .bind(&c.pix_copia_cola)
        .bind(&c.qr_code_b64)
        .bind(c.expires_at)
        .bind(c.paid_at)
        .bind(&c.last_error)
        .bind(c.base.updated_at)
        .bind(c.base.synced)
        .bind(c.base.company_id)
        .bind(c.base.id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }
}
