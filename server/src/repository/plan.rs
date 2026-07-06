use async_trait::async_trait;
use rust_decimal::Decimal;
use chrono::NaiveDateTime;
use sqlx::prelude::FromRow;
use sqlx::PgPool;
use uuid::Uuid;

use letaf_core::error::CoreError;
use letaf_core::plan::model::Plan;
use letaf_core::plan::repository::PlanRepository;

use super::helpers::map_db;

#[derive(FromRow)]
struct PlanRow {
    id: Uuid,
    name: String,
    amount: Decimal,
    period_months: i32,
    trial_days: i32,
    description: String,
    highlight_label: String,
    active: bool,
    sort_order: i32,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    deleted_at: Option<NaiveDateTime>,
}

impl From<PlanRow> for Plan {
    fn from(r: PlanRow) -> Self {
        Self {
            id: r.id,
            name: r.name,
            amount: r.amount,
            period_months: r.period_months,
            trial_days: r.trial_days,
            description: r.description,
            highlight_label: r.highlight_label,
            active: r.active,
            sort_order: r.sort_order,
            created_at: r.created_at,
            updated_at: r.updated_at,
            deleted_at: r.deleted_at,
        }
    }
}

pub struct PgPlanRepository {
    pool: PgPool,
}

impl PgPlanRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl PlanRepository for PgPlanRepository {
    async fn find_all(&self) -> Result<Vec<Plan>, CoreError> {
        let rows = sqlx::query_as::<_, PlanRow>(
            "SELECT * FROM plans WHERE deleted_at IS NULL ORDER BY sort_order, created_at",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(Plan::from).collect())
    }

    async fn find_active(&self) -> Result<Vec<Plan>, CoreError> {
        let rows = sqlx::query_as::<_, PlanRow>(
            "SELECT * FROM plans WHERE deleted_at IS NULL AND active = TRUE ORDER BY sort_order, created_at",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(rows.into_iter().map(Plan::from).collect())
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<Plan>, CoreError> {
        let row = sqlx::query_as::<_, PlanRow>(
            "SELECT * FROM plans WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(row.map(Plan::from))
    }

    async fn create(&self, plan: &Plan) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO plans (id, name, amount, period_months, trial_days, description, highlight_label, active, sort_order, created_at, updated_at, deleted_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
        )
        .bind(plan.id)
        .bind(&plan.name)
        .bind(plan.amount)
        .bind(plan.period_months)
        .bind(plan.trial_days)
        .bind(&plan.description)
        .bind(&plan.highlight_label)
        .bind(plan.active)
        .bind(plan.sort_order)
        .bind(plan.created_at)
        .bind(plan.updated_at)
        .bind(plan.deleted_at)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn update(&self, plan: &Plan) -> Result<(), CoreError> {
        sqlx::query(
            "UPDATE plans SET name = $1, amount = $2, period_months = $3, trial_days = $4, description = $5, highlight_label = $6, active = $7, sort_order = $8, updated_at = $9
             WHERE id = $10",
        )
        .bind(&plan.name)
        .bind(plan.amount)
        .bind(plan.period_months)
        .bind(plan.trial_days)
        .bind(&plan.description)
        .bind(&plan.highlight_label)
        .bind(plan.active)
        .bind(plan.sort_order)
        .bind(plan.updated_at)
        .bind(plan.id)
        .execute(&self.pool)
        .await
        .map_err(map_db)?;
        Ok(())
    }

    async fn soft_delete(&self, id: Uuid) -> Result<(), CoreError> {
        let now = chrono::Utc::now().naive_utc();
        sqlx::query("UPDATE plans SET deleted_at = $1, updated_at = $1 WHERE id = $2 AND deleted_at IS NULL")
            .bind(now)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(map_db)?;
        Ok(())
    }
}
