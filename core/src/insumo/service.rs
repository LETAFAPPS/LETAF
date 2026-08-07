//! Orquestração de regras de Insumo (AI_RULES §1/§9/§11). O handler passa
//! dados brutos; aqui validamos, construímos a entidade e cuidamos de
//! timestamps/synced. Espelha `ProductService` no essencial.

use std::sync::Arc;

use chrono::NaiveDateTime;
use rust_decimal::Decimal;
use uuid::Uuid;

use super::model::{Insumo, InsumoMovement};
use super::repository::InsumoRepository;
use crate::error::CoreError;
use crate::product::repository::StockAdjustResult;

pub struct InsumoService {
    repo: Arc<dyn InsumoRepository>,
}

impl InsumoService {
    pub fn new(repo: Arc<dyn InsumoRepository>) -> Self {
        Self { repo }
    }

    pub async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Insumo>, CoreError> {
        self.repo.find_by_id(company_id, id).await
    }

    pub async fn find_all(&self, company_id: Uuid) -> Result<Vec<Insumo>, CoreError> {
        self.repo.find_all(company_id).await
    }

    /// Valida os dados comuns de um insumo (nome, unidade, não-negativos).
    fn validate(name: &str, unit: &str, stock: f64, min_stock: f64, cost: Option<Decimal>) -> Result<(), CoreError> {
        if name.trim().is_empty() {
            return Err(CoreError::Validation("Informe o nome do insumo".into()));
        }
        if unit.trim().is_empty() {
            return Err(CoreError::Validation("Informe a unidade do insumo".into()));
        }
        if stock < 0.0 {
            return Err(CoreError::Validation("A quantidade em estoque não pode ser negativa".into()));
        }
        if min_stock < 0.0 {
            return Err(CoreError::Validation("O estoque mínimo não pode ser negativo".into()));
        }
        if let Some(c) = cost {
            if c < Decimal::ZERO {
                return Err(CoreError::Validation("O custo não pode ser negativo".into()));
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        &self,
        company_id: Uuid,
        name: String,
        description: Option<String>,
        unit: String,
        stock_quantity: f64,
        min_stock: f64,
        cost_price: Option<Decimal>,
        barcode: Option<String>,
        active: bool,
    ) -> Result<Insumo, CoreError> {
        Self::validate(&name, &unit, stock_quantity, min_stock, cost_price)?;
        let mut insumo = Insumo::new(company_id, name, unit);
        insumo.description = description;
        insumo.stock_quantity = stock_quantity;
        insumo.min_stock = min_stock;
        insumo.cost_price = cost_price;
        insumo.barcode = barcode;
        insumo.active = active;
        self.repo.create(&insumo).await?;
        Ok(insumo)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update(
        &self,
        company_id: Uuid,
        id: Uuid,
        name: String,
        description: Option<String>,
        unit: String,
        stock_quantity: f64,
        min_stock: f64,
        cost_price: Option<Decimal>,
        barcode: Option<String>,
        active: bool,
    ) -> Result<Insumo, CoreError> {
        Self::validate(&name, &unit, stock_quantity, min_stock, cost_price)?;
        let mut insumo = self
            .repo
            .find_by_id(company_id, id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Insumo não encontrado".into()))?;
        let old_stock = insumo.stock_quantity;
        insumo.name = name;
        insumo.description = description;
        insumo.unit = unit;
        insumo.stock_quantity = stock_quantity;
        insumo.min_stock = min_stock;
        insumo.cost_price = cost_price;
        insumo.barcode = barcode;
        insumo.active = active;
        insumo.base.updated_at = chrono::Utc::now().naive_utc();
        insumo.base.synced = false;
        // Estoque NÃO sincroniza por LWW (§7): a diferença vira delta no ledger.
        // Quando há delta, o UPDATE de metadados mantém o estoque ANTIGO e o
        // delta é aplicado à parte — senão a baixa seria contada em dobro.
        let target_stock = stock_quantity;
        let stock_delta = target_stock - old_stock;
        if stock_delta.abs() > f64::EPSILON {
            insumo.stock_quantity = old_stock;
        }
        self.repo.update_atomic(&insumo, stock_delta).await?;
        insumo.stock_quantity = target_stock;
        Ok(insumo)
    }

    pub async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        self.repo
            .find_by_id(company_id, id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Insumo não encontrado".into()))?;
        self.repo.soft_delete(company_id, id).await
    }

    /// Ajuste manual de estoque (entrada/saída) — grava movimento "adjust".
    pub async fn adjust_stock(&self, company_id: Uuid, id: Uuid, delta: f64) -> Result<(), CoreError> {
        self.apply_stock_delta(company_id, id, delta, "adjust").await
    }

    /// Consumo de funcionário: baixa `qty` (positivo) com reason "consumo".
    pub async fn register_consumption(&self, company_id: Uuid, id: Uuid, qty: f64) -> Result<(), CoreError> {
        if qty <= 0.0 {
            return Err(CoreError::Validation("Quantidade de consumo inválida".into()));
        }
        self.apply_stock_delta(company_id, id, -qty, "consumo").await
    }

    async fn apply_stock_delta(&self, company_id: Uuid, id: Uuid, delta: f64, reason: &str) -> Result<(), CoreError> {
        match self.repo.try_adjust_stock(company_id, id, delta, reason).await? {
            StockAdjustResult::Adjusted | StockAdjustResult::Unlimited => Ok(()),
            StockAdjustResult::Insufficient => {
                Err(CoreError::Validation("Estoque insuficiente para a baixa".into()))
            }
            StockAdjustResult::NotFound => Err(CoreError::NotFound("Insumo não encontrado".into())),
        }
    }

    // ── Sincronização ────────────────────────────────────────────────
    pub async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Insumo>, CoreError> {
        self.repo.find_unsynced(company_id).await
    }
    pub async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: NaiveDateTime) -> Result<(), CoreError> {
        self.repo.mark_synced(company_id, id, updated_at).await
    }
    pub async fn sync_upsert(&self, company_id: Uuid, mut insumo: Insumo) -> Result<(), CoreError> {
        if insumo.base.company_id != company_id {
            return Err(CoreError::Validation("Operação não permitida para esta empresa".into()));
        }
        insumo.base.synced = true;
        self.repo.sync_upsert(&insumo).await
    }
    pub async fn find_updated_since_paged(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
        after_id: Uuid,
        limit: i64,
    ) -> Result<Vec<Insumo>, CoreError> {
        self.repo.find_updated_since_paged(company_id, since, after_id, limit).await
    }

    // ── Ledger de movimentos ─────────────────────────────────────────
    pub async fn find_unsynced_movements(&self, company_id: Uuid) -> Result<Vec<InsumoMovement>, CoreError> {
        self.repo.find_unsynced_movements(company_id).await
    }
    pub async fn mark_movement_synced(&self, company_id: Uuid, id: Uuid, updated_at: NaiveDateTime) -> Result<(), CoreError> {
        self.repo.mark_movement_synced(company_id, id, updated_at).await
    }
    pub async fn apply_movement(&self, company_id: Uuid, movement: InsumoMovement) -> Result<(), CoreError> {
        if movement.base.company_id != company_id {
            return Err(CoreError::Validation("Operação não permitida para esta empresa".into()));
        }
        self.repo.apply_movement(&movement).await
    }
    pub async fn sync_insert_movement(&self, company_id: Uuid, movement: InsumoMovement) -> Result<(), CoreError> {
        if movement.base.company_id != company_id {
            return Err(CoreError::Validation("Operação não permitida para esta empresa".into()));
        }
        self.repo.insert_synced_movement(&movement).await
    }
    pub async fn find_movements_updated_since_paged(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
        after_id: Uuid,
        limit: i64,
    ) -> Result<Vec<InsumoMovement>, CoreError> {
        self.repo.find_movements_updated_since_paged(company_id, since, after_id, limit).await
    }
}
