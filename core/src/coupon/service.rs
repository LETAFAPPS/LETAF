use std::sync::Arc;

use chrono::NaiveDateTime;
use uuid::Uuid;

use super::model::{Coupon, COUPON_TYPES, DISCOUNT_KINDS};
use super::repository::CouponRepository;
use crate::error::CoreError;

/// Service para o domínio Coupon.
///
/// Regras aplicadas (AI_RULES.md §1, §9, §11):
/// - Orquestra regras de negócio (validações + repo).
/// - Não confia no frontend: `coupon_type`/`discount_kind` validados
///   contra allowlists; valores monetários/limites sanitizados.
/// - `code` normalizado (UPPER, sem espaços) e único por empresa.
pub struct CouponService {
    repo: Arc<dyn CouponRepository>,
}

#[allow(clippy::too_many_arguments)]
impl CouponService {
    pub fn new(repo: Arc<dyn CouponRepository>) -> Self {
        Self { repo }
    }

    pub async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Coupon>, CoreError> {
        self.repo.find_by_id(company_id, id).await
    }

    pub async fn find_all(&self, company_id: Uuid) -> Result<Vec<Coupon>, CoreError> {
        self.repo.find_all(company_id).await
    }

    pub async fn find_active(&self, company_id: Uuid) -> Result<Vec<Coupon>, CoreError> {
        self.repo.find_active(company_id).await
    }

    /// Avalia um código de cupom para um checkout e devolve
    /// `(Coupon, desconto)` se aplicável, ou um erro de validação com
    /// mensagem amigável (pt-BR).
    ///
    /// Regras aplicadas (AI_RULES.md §1, §11):
    /// - Toda regra de aceitação fica aqui (core), nunca no frontend.
    /// - Limites de uso são contados pelo caller a partir dos pedidos
    ///   (a entidade Order é o registro de uso — sem entidade extra).
    #[allow(clippy::too_many_arguments)]
    pub async fn evaluate(
        &self,
        company_id: Uuid,
        code: &str,
        subtotal: f64,
        now: NaiveDateTime,
        customer_prior_orders: i64,
        total_uses: i64,
        user_uses: i64,
    ) -> Result<(Coupon, f64), CoreError> {
        let code = code.trim()
            .chars().filter(|c| !c.is_whitespace()).collect::<String>()
            .to_uppercase();
        if code.is_empty() {
            return Err(CoreError::Validation("Informe um código de cupom".into()));
        }
        let coupon = self.repo.find_by_code(company_id, &code).await?
            .filter(|c| c.active)
            .ok_or_else(|| CoreError::Validation("Cupom inválido ou inativo".into()))?;

        if let Some(from) = coupon.valid_from {
            if now < from {
                return Err(CoreError::Validation("Cupom ainda não está válido".into()));
            }
        }
        if let Some(until) = coupon.valid_until {
            if now > until {
                return Err(CoreError::Validation("Cupom expirado".into()));
            }
        }
        if coupon.min_order_value > 0.0 && subtotal < coupon.min_order_value {
            return Err(CoreError::Validation(format!(
                "Pedido mínimo de R$ {:.2} para usar este cupom",
                coupon.min_order_value
            )));
        }
        if coupon.usage_limit > 0 && total_uses >= coupon.usage_limit as i64 {
            return Err(CoreError::Validation("Cupom esgotado".into()));
        }
        if coupon.per_user_limit > 0 && user_uses >= coupon.per_user_limit as i64 {
            return Err(CoreError::Validation(
                "Você já atingiu o limite de uso deste cupom".into(),
            ));
        }
        if coupon.coupon_type == "first_purchase" && customer_prior_orders > 0 {
            return Err(CoreError::Validation(
                "Cupom válido apenas na primeira compra".into(),
            ));
        }
        let discount = coupon.discount_for(subtotal);
        Ok((coupon, discount))
    }

    pub async fn create(
        &self,
        company_id: Uuid,
        title: String,
        code: String,
        coupon_type: String,
        discount_kind: String,
        discount_value: f64,
        min_order_value: f64,
        max_discount: f64,
        per_user_limit: i32,
        usage_limit: i32,
        valid_from: Option<NaiveDateTime>,
        valid_until: Option<NaiveDateTime>,
    ) -> Result<Coupon, CoreError> {
        let code = normalize_code(&code);
        validate(
            &title, &code, &coupon_type, &discount_kind, discount_value,
            min_order_value, max_discount, per_user_limit, usage_limit,
            valid_from, valid_until,
        )?;
        // Unicidade do código por empresa.
        if self.repo.find_by_code(company_id, &code).await?.is_some() {
            return Err(CoreError::Validation(format!(
                "Já existe um cupom com o código '{code}'"
            )));
        }
        let coupon = Coupon::new(
            company_id, title, code, coupon_type, discount_kind, discount_value,
            min_order_value, max_discount, per_user_limit, usage_limit,
            valid_from, valid_until,
        );
        self.repo.create(&coupon).await?;
        Ok(coupon)
    }

    pub async fn update(
        &self,
        company_id: Uuid,
        id: Uuid,
        title: String,
        code: String,
        coupon_type: String,
        discount_kind: String,
        discount_value: f64,
        min_order_value: f64,
        max_discount: f64,
        per_user_limit: i32,
        usage_limit: i32,
        valid_from: Option<NaiveDateTime>,
        valid_until: Option<NaiveDateTime>,
    ) -> Result<Coupon, CoreError> {
        let code = normalize_code(&code);
        validate(
            &title, &code, &coupon_type, &discount_kind, discount_value,
            min_order_value, max_discount, per_user_limit, usage_limit,
            valid_from, valid_until,
        )?;
        // Código único: aceita o próprio cupom em edição.
        if let Some(existing) = self.repo.find_by_code(company_id, &code).await? {
            if existing.base.id != id {
                return Err(CoreError::Validation(format!(
                    "Já existe um cupom com o código '{code}'"
                )));
            }
        }
        let mut coupon = self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("Coupon not found".into()))?;
        coupon.title = title;
        coupon.code = code;
        coupon.coupon_type = coupon_type;
        coupon.discount_kind = discount_kind;
        coupon.discount_value = discount_value;
        coupon.min_order_value = min_order_value;
        coupon.max_discount = max_discount;
        coupon.per_user_limit = per_user_limit;
        coupon.usage_limit = usage_limit;
        coupon.valid_from = valid_from;
        coupon.valid_until = valid_until;
        coupon.base.updated_at = chrono::Utc::now().naive_utc();
        coupon.base.synced = false;
        self.repo.update(&coupon).await?;
        Ok(coupon)
    }

    pub async fn set_active(
        &self,
        company_id: Uuid,
        id: Uuid,
        active: bool,
    ) -> Result<(), CoreError> {
        self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("Coupon not found".into()))?;
        self.repo.set_active(company_id, id, active).await
    }

    pub async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("Coupon not found".into()))?;
        self.repo.soft_delete(company_id, id).await
    }

    pub async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Coupon>, CoreError> {
        self.repo.find_unsynced(company_id).await
    }

    pub async fn mark_synced(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        self.repo.mark_synced(company_id, id).await
    }

    pub async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
    ) -> Result<Vec<Coupon>, CoreError> {
        self.repo.find_updated_since(company_id, since).await
    }

    pub async fn sync_upsert(
        &self,
        company_id: Uuid,
        mut coupon: Coupon,
    ) -> Result<(), CoreError> {
        if coupon.base.company_id != company_id {
            return Err(CoreError::Validation("Company mismatch".into()));
        }
        coupon.base.synced = true;
        self.repo.sync_upsert(&coupon).await
    }
}

/// Normaliza o código: trim, sem espaços internos, MAIÚSCULAS.
fn normalize_code(raw: &str) -> String {
    raw.trim()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_uppercase()
}

/// Validação central usada em create e update.
#[allow(clippy::too_many_arguments)]
fn validate(
    title: &str,
    code: &str,
    coupon_type: &str,
    discount_kind: &str,
    discount_value: f64,
    min_order_value: f64,
    max_discount: f64,
    per_user_limit: i32,
    usage_limit: i32,
    valid_from: Option<NaiveDateTime>,
    valid_until: Option<NaiveDateTime>,
) -> Result<(), CoreError> {
    if title.trim().is_empty() {
        return Err(CoreError::Validation("Título do cupom é obrigatório".into()));
    }
    if code.is_empty() {
        return Err(CoreError::Validation("Código do cupom é obrigatório".into()));
    }
    if code.len() > 32 {
        return Err(CoreError::Validation("Código deve ter no máximo 32 caracteres".into()));
    }
    if !COUPON_TYPES.contains(&coupon_type) {
        return Err(CoreError::Validation(format!(
            "Tipo de cupom inválido '{coupon_type}'"
        )));
    }
    if !DISCOUNT_KINDS.contains(&discount_kind) {
        return Err(CoreError::Validation(format!(
            "Tipo de desconto inválido '{discount_kind}'"
        )));
    }
    // Frete grátis não exige valor de desconto.
    if coupon_type != "free_shipping" {
        if discount_value <= 0.0 {
            return Err(CoreError::Validation("Valor do desconto deve ser maior que zero".into()));
        }
        if discount_kind == "percent" && discount_value > 100.0 {
            return Err(CoreError::Validation("Porcentagem não pode ser maior que 100".into()));
        }
    }
    if min_order_value < 0.0 {
        return Err(CoreError::Validation("Valor mínimo de compra não pode ser negativo".into()));
    }
    if max_discount < 0.0 {
        return Err(CoreError::Validation("Desconto máximo não pode ser negativo".into()));
    }
    if per_user_limit < 0 {
        return Err(CoreError::Validation("Limite por usuário não pode ser negativo".into()));
    }
    if usage_limit < 0 {
        return Err(CoreError::Validation("Limite total não pode ser negativo".into()));
    }
    if let (Some(from), Some(until)) = (valid_from, valid_until) {
        if from > until {
            return Err(CoreError::Validation(
                "Início da validade não pode ser depois do fim".into(),
            ));
        }
    }
    Ok(())
}
