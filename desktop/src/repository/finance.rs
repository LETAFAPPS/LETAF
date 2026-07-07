use async_trait::async_trait;
use rust_decimal::prelude::ToPrimitive;
use chrono::{NaiveDate, NaiveDateTime, Utc};
use sqlx::prelude::FromRow;
use sqlx::{Sqlite, SqlitePool, Transaction};
use uuid::Uuid;

use letaf_core::error::CoreError;
use letaf_core::finance::model::{
    FinanceEntry, FinanceKind, FinanceRecurrence, FinanceStatus, PartyType,
};
use letaf_core::finance::repository::FinanceRepository;

use super::helpers::{parse_base, date_str, map_db, parse_date, parse_timestamp, parse_uuid, ts};

#[derive(FromRow)]
struct FinanceEntryRow {
    id: String,
    company_id: String,
    kind: String,
    description: String,
    party_id: Option<String>,
    party_name: String,
    party_type: String,
    category_id: Option<String>,
    amount: f64,
    due_date: String,
    paid_at: Option<String>,
    status: String,
    payment_method: Option<String>,
    notes: Option<String>,
    recurrence: String,
    parent_id: String,
    installment_index: i64,
    installment_total: i64,
    order_id: Option<String>,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    synced: bool,
}

impl TryFrom<FinanceEntryRow> for FinanceEntry {
    type Error = CoreError;
    fn try_from(r: FinanceEntryRow) -> Result<Self, Self::Error> {
        Ok(Self {
            base: parse_base(&r.id, &r.company_id, &r.created_at, &r.updated_at, r.deleted_at.as_deref(), r.synced)?,
            kind: FinanceKind::from_str(&r.kind),
            description: r.description,
            party_id: r.party_id.as_deref().map(parse_uuid).transpose()?,
            party_name: r.party_name,
            party_type: PartyType::from_str(&r.party_type),
            category_id: r.category_id.as_deref().map(parse_uuid).transpose()?,
            amount: letaf_core::money::from_db_f64(r.amount),
            due_date: parse_date(&r.due_date)?,
            paid_at: r.paid_at.as_deref().map(parse_timestamp).transpose()?,
            status: FinanceStatus::from_str(&r.status),
            payment_method: r.payment_method,
            notes: r.notes,
            recurrence: FinanceRecurrence::from_str(&r.recurrence),
            parent_id: parse_uuid(&r.parent_id)?,
            installment_index: r.installment_index as i32,
            installment_total: r.installment_total as i32,
            order_id: r.order_id.as_deref().map(parse_uuid).transpose()?,
        })
    }
}

pub struct SqliteFinanceRepository {
    pool: SqlitePool,
}

impl SqliteFinanceRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl FinanceRepository for SqliteFinanceRepository {
    async fn find_by_id(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<Option<FinanceEntry>, CoreError> {
        let row = sqlx::query_as::<_, FinanceEntryRow>(
            "SELECT * FROM finance_entries
             WHERE company_id = ? AND id = ? AND deleted_at IS NULL",
        )
        .bind(company_id.to_string())
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        row.map(FinanceEntry::try_from).transpose()
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<FinanceEntry>, CoreError> {
        let rows = sqlx::query_as::<_, FinanceEntryRow>(
            "SELECT * FROM finance_entries
             WHERE company_id = ? AND deleted_at IS NULL
             ORDER BY due_date ASC, created_at ASC",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(FinanceEntry::try_from).collect()
    }

    async fn find_by_kind(
        &self,
        company_id: Uuid,
        kind: FinanceKind,
    ) -> Result<Vec<FinanceEntry>, CoreError> {
        let rows = sqlx::query_as::<_, FinanceEntryRow>(
            "SELECT * FROM finance_entries
             WHERE company_id = ? AND kind = ? AND deleted_at IS NULL
             ORDER BY due_date ASC, created_at ASC",
        )
        .bind(company_id.to_string())
        .bind(kind.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(FinanceEntry::try_from).collect()
    }

    async fn find_in_range(
        &self,
        company_id: Uuid,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Vec<FinanceEntry>, CoreError> {
        let rows = sqlx::query_as::<_, FinanceEntryRow>(
            "SELECT * FROM finance_entries
             WHERE company_id = ? AND deleted_at IS NULL
               AND due_date BETWEEN ? AND ?
             ORDER BY due_date ASC",
        )
        .bind(company_id.to_string())
        .bind(date_str(start))
        .bind(date_str(end))
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(FinanceEntry::try_from).collect()
    }

    async fn create(&self, e: &FinanceEntry) -> Result<(), CoreError> {
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        insert_one(&mut tx, e).await?;
        tx.commit().await.map_err(map_db)?;
        Ok(())
    }

    async fn create_batch(&self, entries: &[FinanceEntry]) -> Result<(), CoreError> {
        // AI_RULES §4.Transações: parcelas/recorrência inseridas em
        // uma única transação — ou todas existem ou nenhuma.
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
               kind = ?, description = ?, party_id = ?, party_name = ?,
               party_type = ?, category_id = ?, amount = ?, due_date = ?,
               paid_at = ?, status = ?, payment_method = ?, notes = ?,
               recurrence = ?, parent_id = ?, installment_index = ?,
               installment_total = ?, order_id = ?,
               updated_at = ?, deleted_at = ?, synced = ?
             WHERE company_id = ? AND id = ?",
        )
        .bind(e.kind.to_string())
        .bind(&e.description)
        .bind(e.party_id.map(|u| u.to_string()))
        .bind(&e.party_name)
        .bind(e.party_type.to_string())
        .bind(e.category_id.map(|u| u.to_string()))
        .bind(e.amount.to_f64().unwrap_or(0.0))
        .bind(date_str(e.due_date))
        .bind(e.paid_at.map(ts))
        .bind(e.status.to_string())
        .bind(&e.payment_method)
        .bind(&e.notes)
        .bind(e.recurrence.to_string())
        .bind(e.parent_id.to_string())
        .bind(e.installment_index as i64)
        .bind(e.installment_total as i64)
        .bind(e.order_id.map(|u| u.to_string()))
        .bind(ts(e.base.updated_at))
        .bind(e.base.deleted_at.map(ts))
        .bind(e.base.synced)
        .bind(e.base.company_id.to_string())
        .bind(e.base.id.to_string())
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let now = Utc::now().naive_utc();
        sqlx::query(
            "UPDATE finance_entries
               SET deleted_at = ?, updated_at = ?, synced = 0
             WHERE company_id = ? AND id = ?",
        )
        .bind(ts(now))
        .bind(ts(now))
        .bind(company_id.to_string())
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    // ── Sync ──

    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<FinanceEntry>, CoreError> {
        let rows = sqlx::query_as::<_, FinanceEntryRow>(
            "SELECT * FROM finance_entries WHERE company_id = ? AND synced = 0",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(FinanceEntry::try_from).collect()
    }

    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        sqlx::query("UPDATE finance_entries SET synced = 1 WHERE company_id = ? AND id = ? AND updated_at = ?")
            .bind(company_id.to_string())
            .bind(id.to_string())
            .bind(ts(updated_at))
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
        let rows = sqlx::query_as::<_, FinanceEntryRow>(
            "SELECT * FROM finance_entries WHERE company_id = ? AND updated_at > ?",
        )
        .bind(company_id.to_string())
        .bind(ts(since))
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        rows.into_iter().map(FinanceEntry::try_from).collect()
    }

    async fn sync_upsert(&self, e: &FinanceEntry) -> Result<(), CoreError> {
        // UPSERT com guard de updated_at (last-write-wins, AI_RULES §7.7).
        sqlx::query(
            "INSERT INTO finance_entries
             (id, company_id, kind, description, party_id, party_name,
              party_type, category_id, amount, due_date, paid_at, status,
              payment_method, notes, recurrence, parent_id,
              installment_index, installment_total, order_id,
              created_at, updated_at, deleted_at, synced)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
               kind = excluded.kind,
               description = excluded.description,
               party_id = excluded.party_id,
               party_name = excluded.party_name,
               party_type = excluded.party_type,
               category_id = excluded.category_id,
               amount = excluded.amount,
               due_date = excluded.due_date,
               paid_at = excluded.paid_at,
               status = excluded.status,
               payment_method = excluded.payment_method,
               notes = excluded.notes,
               recurrence = excluded.recurrence,
               parent_id = excluded.parent_id,
               installment_index = excluded.installment_index,
               installment_total = excluded.installment_total,
               order_id = excluded.order_id,
               updated_at = excluded.updated_at,
               deleted_at = excluded.deleted_at,
               synced = excluded.synced
             WHERE excluded.updated_at > finance_entries.updated_at",
        )
        .bind(e.base.id.to_string())
        .bind(e.base.company_id.to_string())
        .bind(e.kind.to_string())
        .bind(&e.description)
        .bind(e.party_id.map(|u| u.to_string()))
        .bind(&e.party_name)
        .bind(e.party_type.to_string())
        .bind(e.category_id.map(|u| u.to_string()))
        .bind(e.amount.to_f64().unwrap_or(0.0))
        .bind(date_str(e.due_date))
        .bind(e.paid_at.map(ts))
        .bind(e.status.to_string())
        .bind(&e.payment_method)
        .bind(&e.notes)
        .bind(e.recurrence.to_string())
        .bind(e.parent_id.to_string())
        .bind(e.installment_index as i64)
        .bind(e.installment_total as i64)
        .bind(e.order_id.map(|u| u.to_string()))
        .bind(ts(e.base.created_at))
        .bind(ts(e.base.updated_at))
        .bind(e.base.deleted_at.map(ts))
        .bind(e.base.synced)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }
}

/// Inserção crua dentro de uma transação ativa. Extraída pra reuso
/// entre `create` (1 entrada) e `create_batch` (N entradas) sem
/// duplicar SQL nem abrir múltiplas transações.
async fn insert_one(
    tx: &mut Transaction<'_, Sqlite>,
    e: &FinanceEntry,
) -> Result<(), CoreError> {
    sqlx::query(
        "INSERT INTO finance_entries
         (id, company_id, kind, description, party_id, party_name,
          party_type, category_id, amount, due_date, paid_at, status,
          payment_method, notes, recurrence, parent_id,
          installment_index, installment_total, order_id,
          created_at, updated_at, deleted_at, synced)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(e.base.id.to_string())
    .bind(e.base.company_id.to_string())
    .bind(e.kind.to_string())
    .bind(&e.description)
    .bind(e.party_id.map(|u| u.to_string()))
    .bind(&e.party_name)
    .bind(e.party_type.to_string())
    .bind(e.category_id.map(|u| u.to_string()))
    .bind(e.amount.to_f64().unwrap_or(0.0))
    .bind(date_str(e.due_date))
    .bind(e.paid_at.map(ts))
    .bind(e.status.to_string())
    .bind(&e.payment_method)
    .bind(&e.notes)
    .bind(e.recurrence.to_string())
    .bind(e.parent_id.to_string())
    .bind(e.installment_index as i64)
    .bind(e.installment_total as i64)
    .bind(e.order_id.map(|u| u.to_string()))
    .bind(ts(e.base.created_at))
    .bind(ts(e.base.updated_at))
    .bind(e.base.deleted_at.map(ts))
    .bind(e.base.synced)
    .execute(&mut **tx)
    .await
    .map_err(map_db)?;
    Ok(())
}
