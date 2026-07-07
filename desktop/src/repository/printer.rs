use async_trait::async_trait;
use sqlx::prelude::FromRow;
use sqlx::SqlitePool;
use uuid::Uuid;

use letaf_core::error::CoreError;
use letaf_core::printer::model::Printer;
use letaf_core::printer::repository::PrinterRepository;

use super::helpers::{parse_base, map_db, ts};

#[derive(FromRow)]
struct PrinterRow {
    id: String,
    company_id: String,
    name: String,
    kind: String,
    system_name: String,
    is_default: bool,
    paper_width: i32,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    synced: bool,
    /// JSON array de UUIDs (string). Quando o operador limpa as
    /// categorias, gravamos `'[]'` (default da migration) — nunca
    /// `NULL`. Parsing tolera array vazio.
    category_ids: String,
}

impl TryFrom<PrinterRow> for Printer {
    type Error = CoreError;
    fn try_from(r: PrinterRow) -> Result<Self, Self::Error> {
        // JSON inválido vira lista vazia + warning; impressora segue
        // funcional como "catch-all" em vez de falhar o select.
        let category_ids: Vec<Uuid> = parse_category_ids(&r.category_ids);
        Ok(Self {
            base: parse_base(&r.id, &r.company_id, &r.created_at, &r.updated_at, r.deleted_at.as_deref(), r.synced)?,
            name: r.name,
            kind: r.kind,
            system_name: r.system_name,
            is_default: r.is_default,
            paper_width: r.paper_width,
            category_ids,
        })
    }
}

fn parse_category_ids(raw: &str) -> Vec<Uuid> {
    serde_json::from_str::<Vec<String>>(raw)
        .ok()
        .map(|v| v.into_iter().filter_map(|s| Uuid::parse_str(&s).ok()).collect())
        .unwrap_or_default()
}

fn serialize_category_ids(ids: &[Uuid]) -> String {
    let as_strings: Vec<String> = ids.iter().map(|u| u.to_string()).collect();
    serde_json::to_string(&as_strings).unwrap_or_else(|_| "[]".into())
}

pub struct SqlitePrinterRepository {
    pool: SqlitePool,
}

impl SqlitePrinterRepository {
    pub fn new(pool: SqlitePool) -> Self { Self { pool } }
}

#[async_trait]
impl PrinterRepository for SqlitePrinterRepository {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Printer>, CoreError> {
        let row = sqlx::query_as::<_, PrinterRow>(
            "SELECT * FROM printers WHERE company_id = ?1 AND id = ?2 AND deleted_at IS NULL",
        )
        .bind(company_id.to_string())
        .bind(id.to_string())
        .fetch_optional(&self.pool).await.map_err(map_db)?;
        row.map(Printer::try_from).transpose()
    }

    async fn find_all(&self, company_id: Uuid) -> Result<Vec<Printer>, CoreError> {
        let rows = sqlx::query_as::<_, PrinterRow>(
            "SELECT * FROM printers WHERE company_id = ?1 AND deleted_at IS NULL \
             ORDER BY kind ASC, is_default DESC, name ASC",
        )
        .bind(company_id.to_string())
        .fetch_all(&self.pool).await.map_err(map_db)?;
        rows.into_iter().map(Printer::try_from).collect()
    }

    async fn find_default(&self, company_id: Uuid, kind: &str) -> Result<Option<Printer>, CoreError> {
        let row = sqlx::query_as::<_, PrinterRow>(
            "SELECT * FROM printers \
             WHERE company_id = ?1 AND kind = ?2 AND is_default = 1 AND deleted_at IS NULL \
             LIMIT 1",
        )
        .bind(company_id.to_string())
        .bind(kind)
        .fetch_optional(&self.pool).await.map_err(map_db)?;
        row.map(Printer::try_from).transpose()
    }

    async fn find_by_kind(&self, company_id: Uuid, kind: &str) -> Result<Vec<Printer>, CoreError> {
        let rows = sqlx::query_as::<_, PrinterRow>(
            "SELECT * FROM printers \
             WHERE company_id = ?1 AND kind = ?2 AND deleted_at IS NULL \
             ORDER BY is_default DESC, name ASC",
        )
        .bind(company_id.to_string())
        .bind(kind)
        .fetch_all(&self.pool).await.map_err(map_db)?;
        rows.into_iter().map(Printer::try_from).collect()
    }

    async fn create(&self, printer: &Printer) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO printers (id, company_id, name, kind, system_name, is_default, paper_width, category_ids, created_at, updated_at, deleted_at, synced)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        )
        .bind(printer.base.id.to_string())
        .bind(printer.base.company_id.to_string())
        .bind(&printer.name)
        .bind(&printer.kind)
        .bind(&printer.system_name)
        .bind(printer.is_default)
        .bind(printer.paper_width)
        .bind(serialize_category_ids(&printer.category_ids))
        .bind(ts(printer.base.created_at))
        .bind(ts(printer.base.updated_at))
        .bind(printer.base.deleted_at.map(ts))
        .bind(printer.base.synced)
        .execute(&self.pool).await.map_err(map_db)?;
        Ok(())
    }

    async fn update(&self, printer: &Printer) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE printers SET name = ?1, kind = ?2, system_name = ?3, is_default = ?4, paper_width = ?5, category_ids = ?6, updated_at = ?7, synced = ?8
             WHERE company_id = ?9 AND id = ?10 AND deleted_at IS NULL",
        )
        .bind(&printer.name)
        .bind(&printer.kind)
        .bind(&printer.system_name)
        .bind(printer.is_default)
        .bind(printer.paper_width)
        .bind(serialize_category_ids(&printer.category_ids))
        .bind(ts(printer.base.updated_at))
        .bind(printer.base.synced)
        .bind(printer.base.company_id.to_string())
        .bind(printer.base.id.to_string())
        .execute(&self.pool).await.map_err(map_db)?;
        Ok(())
    }

    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        sqlx::query(
            "UPDATE printers SET deleted_at = ?1, updated_at = ?2, synced = 1
             WHERE company_id = ?3 AND id = ?4",
        )
        .bind(ts(now))
        .bind(ts(now))
        .bind(company_id.to_string())
        .bind(id.to_string())
        .execute(&self.pool).await.map_err(map_db)?;
        Ok(())
    }

    /// "1 padrão por tipo" — em uma transação:
    ///   1. desmarca todas as outras do mesmo `kind` na empresa;
    ///   2. marca a impressora `id` como padrão.
    ///
    /// Os dois passos no mesmo `BEGIN/COMMIT` evitam janela onde há
    /// duas padrões simultâneas no mesmo `kind`.
    async fn set_default(&self, company_id: Uuid, id: Uuid, kind: &str) -> Result<(), CoreError> {
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        let now = chrono::Utc::now().naive_utc();
        sqlx::query(
            "UPDATE printers SET is_default = 0, updated_at = ?1
             WHERE company_id = ?2 AND kind = ?3 AND id != ?4 AND deleted_at IS NULL",
        )
        .bind(ts(now))
        .bind(company_id.to_string())
        .bind(kind)
        .bind(id.to_string())
        .execute(&mut *tx).await.map_err(map_db)?;
        sqlx::query(
            "UPDATE printers SET is_default = 1, updated_at = ?1
             WHERE company_id = ?2 AND id = ?3 AND deleted_at IS NULL",
        )
        .bind(ts(now))
        .bind(company_id.to_string())
        .bind(id.to_string())
        .execute(&mut *tx).await.map_err(map_db)?;
        tx.commit().await.map_err(map_db)?;
        Ok(())
    }
}
