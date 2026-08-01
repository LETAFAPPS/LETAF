use std::sync::Arc;
use rust_decimal::Decimal;

use chrono::{Duration, NaiveDate, Utc};
use uuid::Uuid;

use super::model::{
    FinanceEntry, FinanceKind, FinanceRecurrence, FinanceStatus, PartyType,
};
use super::repository::FinanceRepository;
use crate::entity::BaseFields;
use crate::error::CoreError;
use crate::util::add_months;
use crate::money::round2;

/// Marcador (em `notes`) da conta a receber AUTOMÁTICA do fiado —
/// identifica a entrada gerida pela carteira do cliente (criada,
/// atualizada e baixada por [`FinanceService::sync_fiado_receivable`]).
pub const FIADO_AUTO_TAG: &str = "[fiado-auto]";

/// Data-sentinela para lançamento SEM vencimento (fiado não tem data
/// de cobrança). Nunca vira "vencido" e a UI exibe "Sem vencimento".
pub fn fiado_due_sentinel() -> NaiveDate {
    NaiveDate::from_ymd_opt(9999, 12, 31).expect("data fixa válida")
}

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
    pub amount: Decimal,
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
        let entries = self.create_group(p).await?;
        Ok(entries.into_iter().next().expect("grupo tem ao menos o head"))
    }

    /// Igual ao `create`, mas devolve TODAS as entradas criadas.
    ///
    /// Necessário para quem precisa do valor TOTAL do grupo: com parcelas
    /// o head vale só a 1ª parcela e com recorrência há N ocorrências —
    /// quem olhasse apenas o head debitaria a menos (ver a ponte
    /// Financeiro→carteira em `desktop/src/ui/finance/wallet_link.rs`).
    pub async fn create_group(
        &self,
        p: CreateFinanceParams,
    ) -> Result<Vec<FinanceEntry>, CoreError> {
        validate_params(&p)?;
        let entries = build_entries(&p);
        self.repo.create_batch(&entries).await?;
        Ok(entries)
    }

    /// Soma das contas a receber ABERTAS de um cliente, EXCLUINDO a
    /// automática do fiado.
    ///
    /// É a parcela da dívida da carteira que já tem lançamento próprio
    /// no Financeiro — o espelho do fiado a desconta para não cobrar
    /// duas vezes o mesmo dinheiro (ver `sync_fiado_receivable`).
    pub async fn open_customer_receivables_total(
        &self,
        company_id: Uuid,
        customer_id: Uuid,
    ) -> Result<Decimal, CoreError> {
        let total = self
            .repo
            .find_by_kind(company_id, FinanceKind::Receivable)
            .await?
            .into_iter()
            .filter(|e| {
                e.party_id == Some(customer_id)
                    && e.notes.as_deref() != Some(FIADO_AUTO_TAG)
                    && !e.status.is_settled()
                    && e.status != FinanceStatus::Cancelled
                    // Recorrência não é dívida assumida — mesmo recorte
                    // usado ao debitar a carteira (`wallet_link`). Os dois
                    // lados PRECISAM considerar o mesmo conjunto, senão o
                    // espelho desconta o que a carteira não debitou.
                    && e.recurrence == FinanceRecurrence::Once
            })
            .map(|e| e.amount)
            .sum();
        Ok(round2(total))
    }

    /// Mantém a conta a receber AUTOMÁTICA do fiado do cliente em dia.
    ///
    /// Regras (AI_RULES.md §1, §7):
    /// - Dívida > 0 e sem entrada aberta → cria "Fiado — {cliente}"
    ///   (sem vencimento, marcada com [`FIADO_AUTO_TAG`]).
    /// - Dívida > 0 e entrada aberta → atualiza o valor para a dívida
    ///   ATUAL (novos pedidos fiados só aumentam o mesmo lançamento).
    /// - Dívida zerada (cliente pagou via carteira) → baixa como
    ///   Recebido.
    ///
    /// Chamado após cada mudança de saldo da carteira — idempotente.
    /// Espelha no Financeiro a dívida derivada do SALDO da carteira.
    ///
    /// Desconta a parte que já tem conta a receber própria (lançada à mão),
    /// senão o mesmo dinheiro seria cobrado duas vezes. Fórmula única para os
    /// dois chamadores — a operação local de carteira e o pull do sync (§8):
    /// tê-la só no lado da UI fazia o espelho ficar defasado quando o saldo
    /// mudava por venda fiada de OUTRO terminal.
    ///
    /// Devolve a dívida espelhada, que NÃO é `-balance`: quem decide se o
    /// fiado foi quitado precisa deste valor, não do saldo (uma conta a
    /// receber manual em aberto deixa o saldo negativo sem haver fiado).
    pub async fn sync_fiado_from_balance(
        &self,
        company_id: Uuid,
        customer_id: Uuid,
        customer_name: &str,
        balance: Decimal,
    ) -> Result<Decimal, CoreError> {
        let com_conta_propria = self
            .open_customer_receivables_total(company_id, customer_id)
            .await
            .unwrap_or(Decimal::ZERO);
        let debt = (-balance - com_conta_propria).max(Decimal::ZERO);
        self.sync_fiado_receivable(company_id, customer_id, customer_name, debt)
            .await?;
        Ok(debt)
    }

    pub async fn sync_fiado_receivable(
        &self,
        company_id: Uuid,
        customer_id: Uuid,
        customer_name: &str,
        debt: Decimal,
    ) -> Result<(), CoreError> {
        let debt = round2(debt.max(Decimal::ZERO));
        // Todas as contas automáticas deste cliente, encerradas ou não. As
        // encerradas são HISTÓRICO (cada uma é um fiado recebido) e não podem
        // ser reaproveitadas — só a aberta é o espelho da dívida atual.
        let automaticas: Vec<FinanceEntry> = self
            .repo
            .find_by_kind(company_id, FinanceKind::Receivable)
            .await?
            .into_iter()
            .filter(|e| {
                e.party_id == Some(customer_id) && e.notes.as_deref() == Some(FIADO_AUTO_TAG)
            })
            .collect();
        let aberta = automaticas.iter().find(|e| {
            !e.status.is_settled() && e.status != FinanceStatus::Cancelled
        });
        let description = format!("Fiado * {customer_name}");

        match aberta {
            Some(entry) if debt > Decimal::ZERO => {
                let mut entry = entry.clone();
                if entry.amount != debt || entry.description != description {
                    entry.amount = debt;
                    entry.party_name = customer_name.to_string();
                    entry.description = description;
                    entry.base.updated_at = Utc::now().naive_utc();
                    entry.base.synced = false;
                    self.repo.update(&entry).await?;
                }
            }
            Some(entry) => {
                // Dívida zerada pela carteira → recebido.
                self.mark_settled(company_id, entry.base.id, Some("wallet".into()))
                    .await?;
            }
            None if debt > Decimal::ZERO => {
                // Id DETERMINÍSTICO pelo histórico: dois terminais que ainda
                // não se enxergaram derivam o mesmo id e o upsert dedupa, em
                // vez de abrirem duas contas para a mesma dívida.
                let id = crate::deterministic_id::fiado_auto_entry(
                    company_id,
                    customer_id,
                    automaticas.len(),
                );
                let mut entry = FinanceEntry::new(
                    company_id,
                    FinanceKind::Receivable,
                    description,
                    debt,
                    fiado_due_sentinel(),
                );
                entry.base.id = id;
                entry.parent_id = id;
                entry.party_id = Some(customer_id);
                entry.party_name = customer_name.to_string();
                entry.party_type = PartyType::Customer;
                entry.notes = Some(FIADO_AUTO_TAG.to_string());
                self.repo.create(&entry).await?;
            }
            None => {}
        }
        Ok(())
    }

    /// Recebimento PARCIAL de uma conta a receber: abate `amount` do
    /// valor em aberto, mantendo o lançamento Pendente com o restante.
    ///
    /// Regras (AI_RULES.md §11): valida 0 < amount < valor em aberto;
    /// lançamento liquidado/cancelado não recebe abatimento.
    pub async fn receive_partial(
        &self,
        company_id: Uuid,
        id: Uuid,
        amount: Decimal,
    ) -> Result<FinanceEntry, CoreError> {
        let mut entry = self
            .repo
            .find_by_id(company_id, id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Lançamento não encontrado".into()))?;
        if entry.kind != FinanceKind::Receivable {
            return Err(CoreError::Validation(
                "Abatimento parcial só vale para contas a receber".into(),
            ));
        }
        if entry.status.is_settled() || entry.status == FinanceStatus::Cancelled {
            return Err(CoreError::Validation(
                "Lançamento encerrado não recebe abatimento".into(),
            ));
        }
        let amount = round2(amount);
        if amount <= Decimal::ZERO || amount >= entry.amount {
            return Err(CoreError::Validation(
                "O abatimento deve ser maior que zero e menor que o valor da conta".into(),
            ));
        }
        entry.amount = round2(entry.amount - amount);
        entry.base.updated_at = Utc::now().naive_utc();
        entry.base.synced = false;
        self.repo.update(&entry).await?;
        Ok(entry)
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
        // §11: o service é a autoridade. Um lançamento CANCELADO não pode ser
        // "liquidado" (reviveria uma despesa/receita cancelada como paga,
        // corrompendo o fluxo de caixa). Simétrico ao guard de `cancel`.
        if entry.status == FinanceStatus::Cancelled {
            return Err(CoreError::Validation(
                "Lançamento cancelado não pode ser liquidado".into(),
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

    pub async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        self.repo.mark_synced(company_id, id, updated_at).await
    }

    pub async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
    ) -> Result<Vec<FinanceEntry>, CoreError> {
        self.repo.find_updated_since(company_id, since).await
    }

    /// Página do pull por keyset `(updated_at, id)`.
    pub async fn find_updated_since_paged(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
        after_id: Uuid,
        limit: i64,
    ) -> Result<Vec<FinanceEntry>, CoreError> {
        self.repo.find_updated_since_paged(company_id, since, after_id, limit).await
    }

    /// Upsert vindo do sync. Recebe a entidade por valor para poder
    /// marcar `synced = true` e validar o `company_id` contra o do
    /// chamador (AI_RULES.md §11 — nunca confiar no payload).
    /// Edição LOCAL de um lançamento (origem: usuário).
    ///
    /// Diferente do `sync_upsert` — que é o caminho do PULL e marca
    /// `synced = true` —, aqui a entrada fica `synced = false` para o
    /// worker levar a alteração ao servidor (§7.3). Usar `sync_upsert`
    /// numa edição fazia a mudança morrer no SQLite.
    pub async fn update(&self, company_id: Uuid, mut entry: FinanceEntry) -> Result<(), CoreError> {
        if entry.base.company_id != company_id {
            return Err(CoreError::Validation(
                "Operação não permitida para esta empresa".into(),
            ));
        }
        entry.base.updated_at = Utc::now().naive_utc();
        entry.base.synced = false;
        self.repo.update(&entry).await
    }

    pub async fn sync_upsert(
        &self,
        company_id: Uuid,
        mut entry: FinanceEntry,
    ) -> Result<(), CoreError> {
        if entry.base.company_id != company_id {
            return Err(CoreError::Validation("Operação não permitida para esta empresa".into()));
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
    if p.amount <= Decimal::ZERO {
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
        let per = round2(p.amount / rust_decimal::Decimal::from(n));
        let last = round2(p.amount - per * rust_decimal::Decimal::from(n - 1));
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

