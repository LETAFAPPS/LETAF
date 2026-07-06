use std::sync::Arc;

use uuid::Uuid;

use super::model::{Printer, PAPER_WIDTHS, PRINTER_KINDS};
use super::repository::PrinterRepository;
use crate::error::CoreError;

/// Service para o domínio Printer.
///
/// Regras aplicadas (AI_RULES.md §1, §9, §11):
/// - Orquestra regras de negócio (validações + repo).
/// - Não confia no frontend: `kind` e `paper_width` validados contra
///   allowlists do core; `name` e `system_name` não vazios.
/// - "1 padrão por tipo" forçado via `repo.set_default` (transação)
///   após `create`/`update` quando o operador marca `is_default = true`.
pub struct PrinterService {
    repo: Arc<dyn PrinterRepository>,
}

impl PrinterService {
    pub fn new(repo: Arc<dyn PrinterRepository>) -> Self { Self { repo } }

    pub async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Printer>, CoreError> {
        self.repo.find_by_id(company_id, id).await
    }

    pub async fn find_all(&self, company_id: Uuid) -> Result<Vec<Printer>, CoreError> {
        self.repo.find_all(company_id).await
    }

    pub async fn find_default(&self, company_id: Uuid, kind: &str) -> Result<Option<Printer>, CoreError> {
        self.repo.find_default(company_id, kind).await
    }

    pub async fn find_by_kind(&self, company_id: Uuid, kind: &str) -> Result<Vec<Printer>, CoreError> {
        self.repo.find_by_kind(company_id, kind).await
    }

    /// Cria a impressora. Se `is_default = true`, depois do INSERT
    /// chama `set_default` para garantir exclusividade do padrão por
    /// `kind` (não há "atomicidade" entre INSERT + UPDATE porque o
    /// SQLite enfileira ambas no mesmo journal — janela inconsistente
    /// dura microssegundos e os leitores caem na lógica de fallback).
    pub async fn create(
        &self,
        company_id: Uuid,
        name: String,
        kind: String,
        system_name: String,
        is_default: bool,
        paper_width: i32,
        category_ids: Vec<Uuid>,
    ) -> Result<Printer, CoreError> {
        validate(&name, &kind, &system_name, paper_width)?;
        let category_ids = dedup_ids(category_ids);
        let printer = Printer::new(
            company_id,
            name,
            kind.clone(),
            system_name,
            is_default,
            paper_width,
            category_ids,
        );
        self.repo.create(&printer).await?;
        if is_default {
            self.repo.set_default(company_id, printer.base.id, &kind).await?;
        }
        Ok(printer)
    }

    pub async fn update(
        &self,
        company_id: Uuid,
        id: Uuid,
        name: String,
        kind: String,
        system_name: String,
        is_default: bool,
        paper_width: i32,
        category_ids: Vec<Uuid>,
    ) -> Result<Printer, CoreError> {
        validate(&name, &kind, &system_name, paper_width)?;
        let mut printer = self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("Printer not found".into()))?;
        printer.name = name;
        printer.kind = kind.clone();
        printer.system_name = system_name;
        printer.is_default = is_default;
        printer.paper_width = paper_width;
        printer.category_ids = dedup_ids(category_ids);
        printer.base.updated_at = chrono::Utc::now().naive_utc();
        // Sempre `true`: impressora não sincroniza (§7).
        printer.base.synced = true;
        self.repo.update(&printer).await?;
        if is_default {
            self.repo.set_default(company_id, id, &kind).await?;
        }
        Ok(printer)
    }

    pub async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("Printer not found".into()))?;
        self.repo.soft_delete(company_id, id).await
    }

    /// Alterna explicitamente o padrão de um `kind` para esta
    /// impressora (sem editar outros campos). Usado pelo toggle
    /// rápido no listing.
    pub async fn set_default(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let printer = self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("Printer not found".into()))?;
        self.repo.set_default(company_id, id, &printer.kind).await
    }
}

/// Remove duplicatas preservando a ordem de inserção. Usado em
/// `category_ids` para tolerar UI que possa enviar o mesmo ID duas
/// vezes (ex.: toggle rápido); a regra "1 categoria → 1 entrada" é
/// requisito implícito do roteamento (não queremos enviar o mesmo
/// item duas vezes pra mesma impressora).
fn dedup_ids(ids: Vec<Uuid>) -> Vec<Uuid> {
    let mut seen = std::collections::HashSet::new();
    ids.into_iter().filter(|id| seen.insert(*id)).collect()
}

/// Validação central — reutilizada por create e update.
fn validate(name: &str, kind: &str, system_name: &str, paper_width: i32) -> Result<(), CoreError> {
    if name.trim().is_empty() {
        return Err(CoreError::Validation("Printer name is required".into()));
    }
    if system_name.trim().is_empty() {
        return Err(CoreError::Validation("Printer system name is required".into()));
    }
    if !PRINTER_KINDS.contains(&kind) {
        return Err(CoreError::Validation(format!(
            "Unknown printer kind '{kind}' (expected: order | kitchen | fiscal)"
        )));
    }
    if !PAPER_WIDTHS.contains(&paper_width) {
        return Err(CoreError::Validation(format!(
            "Unsupported paper width {paper_width} (expected 58 or 80)"
        )));
    }
    Ok(())
}
