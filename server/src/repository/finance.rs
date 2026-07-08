use async_trait::async_trait;
use rust_decimal::Decimal;
use chrono::{NaiveDate, NaiveDateTime, Utc};
use sqlx::prelude::FromRow;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use letaf_core::entity::BaseFields;
use letaf_core::error::CoreError;
use letaf_core::finance::model::{
    FinanceEntry, FinanceKind, FinanceRecurrence, FinanceStatus, PartyType,
};
use letaf_core::finance::repository::FinanceRepository;

use super::helpers::{keyset_pull_sql, map_db};

#[derive(FromRow)]
struct FinanceEntryRow {
    id: Uuid,
    company_id: Uuid,
    kind: String,
    description: String,
    party_id: Option<Uuid>,
    party_name: String,
    party_type: String,
    category_id: Option<Uuid>,
    amount: Decimal,
    due_date: NaiveDate,
    paid_at: Option<NaiveDateTime>,
    status: String,
    payment_method: Option<String>,
    notes: Option<String>,
    recurrence: String,
    parent_id: Uuid,
    installment_index: i32,
    installment_total: i32,
    order_id: Option<Uuid>,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    deleted_at: Option<NaiveDateTime>,
    synced: bool,
}

impl From<FinanceEntryRow> for FinanceEntry {
    fn from(r: FinanceEntryRow) -> Self {
        Self {
            base: BaseFields {
                id: r.id,
                company_id: r.company_id,
                created_at: r.created_at,
                updated_at: r.updated_at,
                deleted_at: r.deleted_at,
                synced: r.synced,
            },
            kind: FinanceKind::from_str(&r.kind),
            description: r.description,
            party_id: r.party_id,
            party_name: r.party_name,
            party_type: PartyType::from_str(&r.party_type),
            category_id: r.category_id,
            amount: r.amount,
            due_date: r.due_date,
            paid_at: r.paid_at,
            status: FinanceStatus::from_str(&r.status),
            payment_method: r.payment_method,
            notes: r.notes,
            recurrence: FinanceRecurrence::from_str(&r.recurrence),
            parent_id: r.parent_id,
            installment_index: r.installment_index,
            installment_total: r.installment_total,
            order_id: r.order_id,
        }
    }
}

pub struct PgFinanceRepository {
    pool: PgPool,
}

impl PgFinanceRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl FinanceRepository for PgFinanceRepository {
    async fn find_by_id(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<Option<FinanceEntry>, CoreError> {
        Ok(sqlx::query_as::<_, FinanceEntryRow>(
            "SELECT * FROM finance_entries
             WHERE company_id = $1 AND id = $2 AND deleted_at IS NULL",
        )
        .bind(company_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?
        .map(Into::into))
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<FinanceEntry>, CoreError> {
        Ok(sqlx::query_as::<_, FinanceEntryRow>(
            "SELECT * FROM finance_entries
             WHERE company_id = $1 AND deleted_at IS NULL
             ORDER BY due_date ASC, created_at ASC",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?
        .into_iter()
        .map(Into::into)
        .collect())
    }

    async fn find_by_kind(
        &self,
        company_id: Uuid,
        kind: FinanceKind,
    ) -> Result<Vec<FinanceEntry>, CoreError> {
        Ok(sqlx::query_as::<_, FinanceEntryRow>(
            "SELECT * FROM finance_entries
             WHERE company_id = $1 AND kind = $2 AND deleted_at IS NULL
             ORDER BY due_date ASC, created_at ASC",
        )
        .bind(company_id)
        .bind(kind.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?
        .into_iter()
        .map(Into::into)
        .collect())
    }

    async fn find_in_range(
        &self,
        company_id: Uuid,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Vec<FinanceEntry>, CoreError> {
        Ok(sqlx::query_as::<_, FinanceEntryRow>(
            "SELECT * FROM finance_entries
             WHERE company_id = $1 AND deleted_at IS NULL
               AND due_date BETWEEN $2 AND $3
             ORDER BY due_date ASC",
        )
        .bind(company_id)
        .bind(start)
        .bind(end)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?
        .into_iter()
        .map(Into::into)
        .collect())
    }

    async fn create(&self, e: &FinanceEntry) -> Result<(), CoreError> {
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        insert_one(&mut tx, e).await?;
        tx.commit().await.map_err(map_db)?;
        Ok(())
    }

    async fn create_batch(&self, entries: &[FinanceEntry]) -> Result<(), CoreError> {
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        for e in entries {
            insert_one(&mut tx, e).await?;
        }
        tx.commit().await.map_err(map_db)?;
        Ok(())
    }

    async fn update(&self, e: &FinanceEntry) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE finance_entries SET
               kind = $1, description = $2, party_id = $3, party_name = $4,
               party_type = $5, category_id = $6, amount = $7, due_date = $8,
               paid_at = $9, status = $10, payment_method = $11, notes = $12,
               recurrence = $13, parent_id = $14, installment_index = $15,
               installment_total = $16, order_id = $17,
               updated_at = $18, deleted_at = $19, synced = $20
             WHERE company_id = $21 AND id = $22",
        )
        .bind(e.kind.to_string())
        .bind(&e.description)
        .bind(e.party_id)
        .bind(&e.party_name)
        .bind(e.party_type.to_string())
        .bind(e.category_id)
        .bind(e.amount)
        .bind(e.due_date)
        .bind(e.paid_at)
        .bind(e.status.to_string())
        .bind(&e.payment_method)
        .bind(&e.notes)
        .bind(e.recurrence.to_string())
        .bind(e.parent_id)
        .bind(e.installment_index)
        .bind(e.installment_total)
        .bind(e.order_id)
        .bind(e.base.updated_at)
        .bind(e.base.deleted_at)
        .bind(e.base.synced)
        .bind(e.base.company_id)
        .bind(e.base.id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let now = Utc::now().naive_utc();
        sqlx::query(
            "UPDATE finance_entries
               SET deleted_at = $1, updated_at = $2, synced = FALSE
             WHERE company_id = $3 AND id = $4",
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

    // ── Sync ──

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<FinanceEntry>, CoreError> {
        Ok(sqlx::query_as::<_, FinanceEntryRow>(
            "SELECT * FROM finance_entries WHERE company_id = $1 AND synced = FALSE",
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?
        .into_iter()
        .map(Into::into)
        .collect())
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE finance_entries SET synced = TRUE
             WHERE company_id = $1 AND id = $2 AND updated_at = $3",
        )
        .bind(company_id)
        .bind(id)
        .bind(updated_at)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
    ) -> Result<Vec<FinanceEntry>, CoreError> {
        Ok(sqlx::query_as::<_, FinanceEntryRow>(
            "SELECT * FROM finance_entries
             WHERE company_id = $1 AND updated_at > $2",
        )
        .bind(company_id)
        .bind(since)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?
        .into_iter()
        .map(Into::into)
        .collect())
    }

    async fn find_updated_since_paged(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
        after_id: Uuid,
        limit: i64,
    ) -> Result<Vec<FinanceEntry>, CoreError> {
        Ok(sqlx::query_as::<_, FinanceEntryRow>(&keyset_pull_sql("finance_entries"))
        .bind(company_id)
        .bind(since)
        .bind(after_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?
        .into_iter()
        .map(Into::into)
        .collect())
    }

    async fn sync_upsert(&self, e: &FinanceEntry) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO finance_entries
             (id, company_id, kind, description, party_id, party_name,
              party_type, category_id, amount, due_date, paid_at, status,
              payment_method, notes, recurrence, parent_id,
              installment_index, installment_total, order_id,
              created_at, updated_at, deleted_at, synced)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12,
                     $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23)
             ON CONFLICT (id) DO UPDATE SET
               kind = EXCLUDED.kind,
               description = EXCLUDED.description,
               party_id = EXCLUDED.party_id,
               party_name = EXCLUDED.party_name,
               party_type = EXCLUDED.party_type,
               category_id = EXCLUDED.category_id,
               amount = EXCLUDED.amount,
               due_date = EXCLUDED.due_date,
               paid_at = EXCLUDED.paid_at,
               status = EXCLUDED.status,
               payment_method = EXCLUDED.payment_method,
               notes = EXCLUDED.notes,
               recurrence = EXCLUDED.recurrence,
               parent_id = EXCLUDED.parent_id,
               installment_index = EXCLUDED.installment_index,
               installment_total = EXCLUDED.installment_total,
               order_id = EXCLUDED.order_id,
               updated_at = EXCLUDED.updated_at,
               deleted_at = EXCLUDED.deleted_at,
               synced = EXCLUDED.synced
             WHERE EXCLUDED.updated_at > finance_entries.updated_at AND finance_entries.company_id = EXCLUDED.company_id",
        )
        .bind(e.base.id)
        .bind(e.base.company_id)
        .bind(e.kind.to_string())
        .bind(&e.description)
        .bind(e.party_id)
        .bind(&e.party_name)
        .bind(e.party_type.to_string())
        .bind(e.category_id)
        .bind(e.amount)
        .bind(e.due_date)
        .bind(e.paid_at)
        .bind(e.status.to_string())
        .bind(&e.payment_method)
        .bind(&e.notes)
        .bind(e.recurrence.to_string())
        .bind(e.parent_id)
        .bind(e.installment_index)
        .bind(e.installment_total)
        .bind(e.order_id)
        .bind(e.base.created_at)
        .bind(e.base.updated_at)
        .bind(e.base.deleted_at)
        .bind(e.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }
}

/// Inserção crua dentro de uma transação ativa. Mesmo motivo do
/// `desktop/src/repository/finance.rs`: reuso entre create singular
/// e create_batch sem duplicar SQL.
async fn insert_one(
    tx: &mut Transaction<'_, Postgres>,
    e: &FinanceEntry,
) -> Result<(), CoreError> {
    sqlx::query(
        "INSERT INTO finance_entries
         (id, company_id, kind, description, party_id, party_name,
          party_type, category_id, amount, due_date, paid_at, status,
          payment_method, notes, recurrence, parent_id,
          installment_index, installment_total, order_id,
          created_at, updated_at, deleted_at, synced)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12,
                 $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23)",
    )
    .bind(e.base.id)
    .bind(e.base.company_id)
    .bind(e.kind.to_string())
    .bind(&e.description)
    .bind(e.party_id)
    .bind(&e.party_name)
    .bind(e.party_type.to_string())
    .bind(e.category_id)
    .bind(e.amount)
    .bind(e.due_date)
    .bind(e.paid_at)
    .bind(e.status.to_string())
    .bind(&e.payment_method)
    .bind(&e.notes)
    .bind(e.recurrence.to_string())
    .bind(e.parent_id)
    .bind(e.installment_index)
    .bind(e.installment_total)
    .bind(e.order_id)
    .bind(e.base.created_at)
    .bind(e.base.updated_at)
    .bind(e.base.deleted_at)
    .bind(e.base.synced)
    .execute(&mut **tx)
    .await
    .map_err(map_db)?;
    Ok(())
}
