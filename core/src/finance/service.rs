use std::sync::Arc;

use chrono::{Duration, NaiveDate, Utc};
use uuid::Uuid;

use super::model::{
    FinanceEntry, FinanceKind, FinanceRecurrence, FinanceStatus, PartyType,
};
use super::repository::FinanceRepository;
use crate::entity::BaseFields;
use crate::error::CoreError;
use crate::util::{add_months, round_2};

/// Parâmetros para criação de um lançamento.
///
/// Encapsulamos no struct pra evitar funções com 12 argumentos
/// (AI_RULES.md §8: funções pequenas, código legível).
pub struct CreateFinanceParams {
    pub company_id: Uuid,
    pub kind: FinanceKind,
    pub description: String,
    pub party_id: Option<Uuid>,
    pub party_name: String,
    pub party_type: PartyType,
    pub category_id: Option<Uuid>,
    pub amount: f64,
    pub due_date: NaiveDate,
    pub payment_method: Option<String>,
    pub notes: Option<String>,
    pub recurrence: FinanceRecurrence,
    pub installments: i32,
    pub order_id: Option<Uuid>,
}

/// Quantas ocorrências futuras pré-geramos para `Weekly` e `Monthly`.
/// 12 = 1 ano de recorrência mensal, ~3 meses de semanal. Quando o
/// último registro for baixado, o service pode estender — fora de
/// escopo desta fase.
const RECURRENCE_OCCURRENCES: i32 = 12;

/// Serviço de lançamentos financeiros.
///
/// Regras aplicadas (AI_RULES.md §1, §11, §14):
/// - Toda validação aqui (nunca confiar no payload).
/// - Geração de parcelas/recorrência é operação composta → cria em
///   transação única via `create_batch` (§4.Transações).
pub struct FinanceService {
    repo: Arc<dyn FinanceRepository>,
}

impl FinanceService {
    pub fn new(repo: Arc<dyn FinanceRepository>) -> Self {
        Self { repo }
    }

    /// Cria um lançamento (e suas parcelas/recorrências, se houver).
    /// Retorna o cabeça do grupo.
    pub async fn create(&self, p: CreateFinanceParams) -> Result<FinanceEntry, CoreError> {
        validate_params(&p)?;
        let entries = build_entries(&p);
        let head = entries[0].clone();
        self.repo.create_batch(&entries).await?;
        Ok(head)
    }

    /// Marca o lançamento como liquidado.
    /// `Paid` para Payable, `Received` para Receivable.
    pub async fn mark_settled(
        &self,
        company_id: Uuid,
        id: Uuid,
        payment_method: Option<String>,
    ) -> Result<FinanceEntry, CoreError> {
        let mut entry = self
            .repo
            .find_by_id(company_id, id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Lançamento não encontrado".into()))?;

        if entry.status.is_settled() {
            return Err(CoreError::Validation(
                "Lançamento já foi liquidado".into(),
            ));
        }
        let now = Utc::now().naive_utc();
        entry.status = match entry.kind {
            FinanceKind::Payable => FinanceStatus::Paid,
            FinanceKind::Receivable => FinanceStatus::Received,
        };
        entry.paid_at = Some(now);
        if payment_method.is_some() {
            entry.payment_method = payment_method;
        }
        entry.base.updated_at = now;
        entry.base.synced = false;
        self.repo.update(&entry).await?;
        Ok(entry)
    }

    /// Cancela um lançamento (estorna se já liquidado fica fora de
    /// escopo desta fase — quem chama deve checar o status antes).
    pub async fn cancel(&self, company_id: Uuid, id: Uuid) -> Result<FinanceEntry, CoreError> {
        let mut entry = self
            .repo
            .find_by_id(company_id, id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Lançamento não encontrado".into()))?;

        if entry.status.is_settled() {
            return Err(CoreError::Validation(
                "Lançamento já liquidado não pode ser cancelado — registre um estorno".into(),
            ));
        }
        entry.status = FinanceStatus::Cancelled;
        let now = Utc::now().naive_utc();
        entry.base.updated_at = now;
        entry.base.synced = false;
        self.repo.update(&entry).await?;
        Ok(entry)
    }

    /// Soft delete.
    pub async fn delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        self.repo.soft_delete(company_id, id).await
    }

    pub async fn find_by_id(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<Option<FinanceEntry>, CoreError> {
        self.repo.find_by_id(company_id, id).await
    }

    pub async fn find_all(&self, company_id: Uuid) -> Result<Vec<FinanceEntry>, CoreError> {
        self.repo.find_all(company_id).await
    }

    pub async fn find_by_kind(
        &self,
        company_id: Uuid,
        kind: FinanceKind,
    ) -> Result<Vec<FinanceEntry>, CoreError> {
        self.repo.find_by_kind(company_id, kind).await
    }

    pub async fn find_in_range(
        &self,
        company_id: Uuid,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Vec<FinanceEntry>, CoreError> {
        self.repo.find_in_range(company_id, start, end).await
    }

    // ── Sync (delegação) ──

    pub async fn find_unsynced(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<FinanceEntry>, CoreError> {
        self.repo.find_unsynced(company_id).await
    }

    pub async fn mark_synced(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        self.repo.mark_synced(company_id, id).await
    }

    pub async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
    ) -> Result<Vec<FinanceEntry>, CoreError> {
        self.repo.find_updated_since(company_id, since).await
    }

    /// Upsert vindo do sync. Recebe a entidade por valor para poder
    /// marcar `synced = true` e validar o `company_id` contra o do
    /// chamador (AI_RULES.md §11 — nunca confiar no payload).
    pub async fn sync_upsert(
        &self,
        company_id: Uuid,
        mut entry: FinanceEntry,
    ) -> Result<(), CoreError> {
        if entry.base.company_id != company_id {
            return Err(CoreError::Validation("Company mismatch".into()));
        }
        entry.base.synced = true;
        self.repo.sync_upsert(&entry).await
    }
}

/// Validações de criação. Falha rápido com mensagem clara.
fn validate_params(p: &CreateFinanceParams) -> Result<(), CoreError> {
    if p.description.trim().is_empty() {
        return Err(CoreError::Validation("Descrição é obrigatória".into()));
    }
    if p.description.chars().count() > 200 {
        return Err(CoreError::Validation(
            "Descrição deve ter no máximo 200 caracteres".into(),
        ));
    }
    if !p.amount.is_finite() || p.amount <= 0.0 {
        return Err(CoreError::Validation(
            "Valor deve ser maior que zero".into(),
        ));
    }
    if p.installments < 1 || p.installments > 60 {
        return Err(CoreError::Validation(
            "Parcelas deve estar entre 1 e 60".into(),
        ));
    }
    // Recorrência + parcelas combinadas viram cilada UX. Bloqueia.
    if !matches!(p.recurrence, FinanceRecurrence::Once) && p.installments > 1 {
        return Err(CoreError::Validation(
            "Não é possível usar parcelamento junto com recorrência".into(),
        ));
    }
    Ok(())
}

/// Monta a lista de entradas a inserir. Casos:
/// - 1 parcela, recorrência `Once`  → 1 entrada.
/// - N parcelas, recorrência `Once` → N entradas com `due_date`
///   somando 1 mês entre cada e `installment_index/total`.
/// - 1 parcela, recorrência `Weekly`/`Monthly` →
///   [`RECURRENCE_OCCURRENCES`] entradas, uma por semana ou mês.
fn build_entries(p: &CreateFinanceParams) -> Vec<FinanceEntry> {
    let head = build_head(p);
    let head_id = head.base.id;
    let mut out = vec![head];

    if p.installments > 1 {
        let n = p.installments;
        // Arredonda a parcela a 2 casas; a ÚLTIMA absorve o resto para a
        // soma bater com o total (ex.: 100,00/3 = 33,33 + 33,33 + 33,34).
        let per = round_2(p.amount / n as f64);
        let last = round_2(p.amount - per * (n - 1) as f64);
        // Atualiza o head para refletir a 1ª parcela.
        if let Some(first) = out.first_mut() {
            first.amount = per;
            first.installment_index = 1;
            first.installment_total = n;
        }
        for i in 2..=n {
            let due = add_months(p.due_date, i - 1);
            let mut child = clone_child(&out[0], head_id, due);
            child.amount = if i == n { last } else { per };
            child.installment_index = i;
            child.installment_total = n;
            out.push(child);
        }
    } else if !matches!(p.recurrence, FinanceRecurrence::Once) {
        for i in 1..RECURRENCE_OCCURRENCES {
            let due = match p.recurrence {
                FinanceRecurrence::Weekly => p.due_date + Duration::weeks(i as i64),
                FinanceRecurrence::Monthly => add_months(p.due_date, i),
                _ => p.due_date,
            };
            let child = clone_child(&out[0], head_id, due);
            out.push(child);
        }
    }

    out
}

/// Constrói o "cabeça" do grupo a partir dos parâmetros. O id do head
/// também ocupa o `parent_id` (convenção para uniformizar queries).
fn build_head(p: &CreateFinanceParams) -> FinanceEntry {
    let mut entry = FinanceEntry::new(
        p.company_id,
        p.kind,
        p.description.trim().to_string(),
        p.amount,
        p.due_date,
    );
    entry.party_id = p.party_id;
    entry.party_name = p.party_name.trim().to_string();
    entry.party_type = p.party_type;
    entry.category_id = p.category_id;
    entry.payment_method = p.payment_method.clone();
    entry.notes = p.notes.clone();
    entry.recurrence = p.recurrence;
    entry.order_id = p.order_id;
    entry
}

/// Clona uma entrada cabeça mantendo os mesmos campos imutáveis e
/// gerando novo id/base. Usado para parcelas e recorrências.
fn clone_child(head: &FinanceEntry, parent_id: Uuid, due_date: NaiveDate) -> FinanceEntry {
    let base = BaseFields::new(head.base.company_id);
    FinanceEntry {
        base,
        kind: head.kind,
        description: head.description.clone(),
        party_id: head.party_id,
        party_name: head.party_name.clone(),
        party_type: head.party_type,
        category_id: head.category_id,
        amount: head.amount,
        due_date,
        paid_at: None,
        status: FinanceStatus::Pending,
        payment_method: head.payment_method.clone(),
        notes: head.notes.clone(),
        recurrence: head.recurrence,
        parent_id,
        installment_index: 1,
        installment_total: 1,
        order_id: head.order_id,
    }
}

