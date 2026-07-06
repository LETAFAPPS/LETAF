use async_trait::async_trait;
use sqlx::prelude::FromRow;
use sqlx::SqlitePool;
use uuid::Uuid;

use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;
use letaf_core::payment_gateway::model::{ChargeStatus, PaymentCharge};
use letaf_core::payment_gateway::repository::PaymentChargeRepository;

use super::helpers::{map_db, parse_timestamp, parse_uuid, ts};

#[derive(FromRow)]
struct PaymentChargeRow {
    id: String,
    company_id: String,
    invoice_id: Option<String>,
    gateway: String,
    method: String,
    txid: Option<String>,
    amount: f64,
    status: String,
    pix_copia_cola: Option<String>,
    qr_code_b64: Option<String>,
    expires_at: Option<String>,
    paid_at: Option<String>,
    last_error: Option<String>,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    synced: bool,
}

impl TryFrom<PaymentChargeRow> for PaymentCharge {
    type Error = CoreError;

    fn try_from(r: PaymentChargeRow) -> Result<Self, Self::Error> {
        Ok(Self {
            base: BaseFields {
                id: parse_uuid(&r.id)?,
                company_id: parse_uuid(&r.company_id)?,
                created_at: parse_timestamp(&r.created_at)?,
                updated_at: parse_timestamp(&r.updated_at)?,
                deleted_at: r.deleted_at.as_deref().map(parse_timestamp).transpose()?,
                synced: r.synced,
            },
            invoice_id: r.invoice_id.as_deref().map(parse_uuid).transpose()?,
            gateway: r.gateway,
            method: r.method,
            txid: r.txid,
            amount: r.amount,
            status: ChargeStatus::from_str(&r.status),
            pix_copia_cola: r.pix_copia_cola,
            qr_code_b64: r.qr_code_b64,
            expires_at: r.expires_at.as_deref().map(parse_timestamp).transpose()?,
            paid_at: r.paid_at.as_deref().map(parse_timestamp).transpose()?,
            last_error: r.last_error,
        })
    }
}

/// SQLite repo de cobranças. Por ora o desktop não persiste cobranças
/// localmente (só consulta o server via HTTP); fica pronto para
/// quando guardar histórico offline fizer sentido.
#[allow(dead_code)]
pub struct SqlitePaymentChargeRepository {
    pool: SqlitePool,
}

impl SqlitePaymentChargeRepository {
    #[allow(dead_code)]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl PaymentChargeRepository for SqlitePaymentChargeRepository {
    async fn find_by_id(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<Option<PaymentCharge>, CoreError> {
        let row = sqlx::query_as::<_, PaymentChargeRow>(
            "SELECT * FROM payment_charges
             WHERE company_id = ?1 AND id = ?2 AND deleted_at IS NULL",
        )
        .bind(company_id.to_string())
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        row.map(PaymentCharge::try_from).transpose()
    }

    async fn find_by_txid(
        &self,
        company_id: Uuid,
        txid: &str,
    ) -> Result<Option<PaymentCharge>, CoreError> {
        let row = sqlx::query_as::<_, PaymentChargeRow>(
            "SELECT * FROM payment_charges
             WHERE company_id = ?1 AND txid = ?2 AND deleted_at IS NULL",
        )
        .bind(company_id.to_string())
        .bind(txid)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        row.map(PaymentCharge::try_from).transpose()
    }

    async fn create(&self, c: &PaymentCharge) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO payment_charges
             (id, company_id, invoice_id, gateway, method, txid, amount, status,
              pix_copia_cola, qr_code_b64, expires_at, paid_at, last_error,
              created_at, updated_at, deleted_at, synced)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
        )
        .bind(c.base.id.to_string())
        .bind(c.base.company_id.to_string())
        .bind(c.invoice_id.map(|x| x.to_string()))
        .bind(&c.gateway)
        .bind(&c.method)
        .bind(&c.txid)
        .bind(c.amount)
        .bind(c.status.as_str())
        .bind(&c.pix_copia_cola)
        .bind(&c.qr_code_b64)
        .bind(c.expires_at.map(ts))
        .bind(c.paid_at.map(ts))
        .bind(&c.last_error)
        .bind(ts(c.base.created_at))
        .bind(ts(c.base.updated_at))
        .bind(c.base.deleted_at.map(ts))
        .bind(c.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn update(&self, c: &PaymentCharge) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE payment_charges
                SET invoice_id = ?1, gateway = ?2, method = ?3, txid = ?4,
                    amount = ?5, status = ?6, pix_copia_cola = ?7, qr_code_b64 = ?8,
                    expires_at = ?9, paid_at = ?10, last_error = ?11,
                    updated_at = ?12, synced = ?13
              WHERE company_id = ?14 AND id = ?15 AND deleted_at IS NULL",
        )
        .bind(c.invoice_id.map(|x| x.to_string()))
        .bind(&c.gateway)
        .bind(&c.method)
        .bind(&c.txid)
        .bind(c.amount)
        .bind(c.status.as_str())
        .bind(&c.pix_copia_cola)
        .bind(&c.qr_code_b64)
        .bind(c.expires_at.map(ts))
        .bind(c.paid_at.map(ts))
        .bind(&c.last_error)
        .bind(ts(c.base.updated_at))
        .bind(c.base.synced)
        .bind(c.base.company_id.to_string())
        .bind(c.base.id.to_string())
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }
}
