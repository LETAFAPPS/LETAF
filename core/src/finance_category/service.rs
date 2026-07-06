use std::sync::Arc;

use chrono::Utc;
use uuid::Uuid;

use super::model::{FinanceCategory, FinanceCategoryScope};
use super::repository::FinanceCategoryRepository;
use crate::error::CoreError;

/// Serviço de categorias financeiras.
///
/// Regras aplicadas (AI_RULES.md §1, §11, §14):
/// - Toda validação acontece aqui (nome não vazio, cor opcional em
///   formato hex, scope válido).
/// - Nunca confiamos no payload da UI/REST.
/// - Acesso a dados apenas via [`FinanceCategoryRepository`] (§10).
pub struct FinanceCategoryService {
    repo: Arc<dyn FinanceCategoryRepository>,
}

impl FinanceCategoryService {
    pub fn new(repo: Arc<dyn FinanceCategoryRepository>) -> Self {
        Self { repo }
    }

    /// Cria uma nova categoria. Validação:
    /// - Nome: trim, não vazio, ≤ 80 chars.
    /// - Cor: vazia (sem cor) ou hex `#RRGGBB`.
    /// - Escopo: vem tipado, sem validação extra.
    pub async fn create(
        &self,
        company_id: Uuid,
        name: String,
        color: String,
        icon: String,
        scope: FinanceCategoryScope,
    ) -> Result<FinanceCategory, CoreError> {
        let name = name.trim().to_string();
        validate_name(&name)?;
        let color = validate_color(color)?;

        let mut entry = FinanceCategory::new(company_id, name);
        entry.color = color;
        entry.icon = icon;
        entry.scope = scope;
        self.repo.create(&entry).await?;
        Ok(entry)
    }

    /// Atualiza uma categoria existente. Mantém os campos `base` e
    /// re-valida os mesmos invariantes.
    pub async fn update(
        &self,
        company_id: Uuid,
        id: Uuid,
        name: String,
        color: String,
        icon: String,
        scope: FinanceCategoryScope,
    ) -> Result<FinanceCategory, CoreError> {
        let mut current = self
            .repo
            .find_by_id(company_id, id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Categoria não encontrada".into()))?;

        let name = name.trim().to_string();
        validate_name(&name)?;
        let color = validate_color(color)?;

        current.name = name;
        current.color = color;
        current.icon = icon;
        current.scope = scope;
        current.base.updated_at = Utc::now().naive_utc();
        current.base.synced = false;
        self.repo.update(&current).await?;
        Ok(current)
    }

    /// Remoção lógica.
    pub async fn delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        self.repo.soft_delete(company_id, id).await
    }

    pub async fn find_by_id(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<Option<FinanceCategory>, CoreError> {
        self.repo.find_by_id(company_id, id).await
    }

    pub async fn find_all(&self, company_id: Uuid) -> Result<Vec<FinanceCategory>, CoreError> {
        self.repo.find_all(company_id).await
    }

    // ── Sync (delegado ao repositório) ──

    pub async fn find_unsynced(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<FinanceCategory>, CoreError> {
        self.repo.find_unsynced(company_id).await
    }

    pub async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        self.repo.mark_synced(company_id, id, updated_at).await
    }

    pub async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
    ) -> Result<Vec<FinanceCategory>, CoreError> {
        self.repo.find_updated_since(company_id, since).await
    }

    /// Upsert vindo do sync. Recebe a entidade por valor para poder
    /// marcar `synced = true` e validar o `company_id` contra o do
    /// chamador (AI_RULES.md §11 — nunca confiar no payload).
    pub async fn sync_upsert(
        &self,
        company_id: Uuid,
        mut category: FinanceCategory,
    ) -> Result<(), CoreError> {
        if category.base.company_id != company_id {
            return Err(CoreError::Validation("Company mismatch".into()));
        }
        category.base.synced = true;
        self.repo.sync_upsert(&category).await
    }

    /// Popula a empresa com categorias padrão **se ainda não houver
    /// nenhuma**. Idempotente: roda sempre no boot do desktop, mas só
    /// cria as seeds na primeira vez. As próximas execuções terminam
    /// no `find_all().is_empty()` check.
    ///
    /// Sem ON CONFLICT na chave natural porque o domínio não tem uma
    /// — categorias podem repetir entre empresas, e dentro da mesma
    /// empresa o usuário pode legitimamente apagar e recriar.
    pub async fn seed_defaults(&self, company_id: Uuid) -> Result<usize, CoreError> {
        if !self.repo.find_all(company_id).await?.is_empty() {
            return Ok(0);
        }
        let mut count = 0;
        for (name, color, icon, scope) in default_seeds() {
            // Re-uso do `create` para passar pelas mesmas validações
            // de nome/cor — qualquer mudança futura nas regras pega
            // a seed também.
            self.create(
                company_id,
                name.to_string(),
                color.to_string(),
                icon.to_string(),
                scope,
            )
            .await?;
            count += 1;
        }
        Ok(count)
    }
}

/// Seeds iniciais — listadas inline porque são pt-BR e ligadas a UX.
/// Mantidas curtas para serem fáceis de auditar/editar.
fn default_seeds() -> Vec<(&'static str, &'static str, &'static str, FinanceCategoryScope)> {
    use FinanceCategoryScope::*;
    vec![
        // Saídas (payable)
        ("Aluguel",          "#E65100", "", Payable),
        ("Insumos",          "#8E24AA", "", Payable),
        ("Salários",         "#1E88E5", "", Payable),
        ("Impostos",         "#C62828", "", Payable),
        ("Energia",          "#F9A825", "", Payable),
        ("Internet/Telefone","#00897B", "", Payable),
        ("Marketing",        "#D81B60", "", Payable),
        // Entradas (receivable)
        ("Venda",            "#2E7D32", "", Receivable),
        ("Serviço",          "#43A047", "", Receivable),
        ("Mensalidade",      "#558B2F", "", Receivable),
        // Ambos
        ("Outros",           "#607D8B", "", Both),
        ("Ajuste Manual",    "#455A64", "", Both),
    ]
}

/// Nome obrigatório, ≤ 80 chars.
fn validate_name(name: &str) -> Result<(), CoreError> {
    if name.is_empty() {
        return Err(CoreError::Validation(
            "Nome da categoria é obrigatório".into(),
        ));
    }
    if name.chars().count() > 80 {
        return Err(CoreError::Validation(
            "Nome da categoria deve ter no máximo 80 caracteres".into(),
        ));
    }
    Ok(())
}

/// Cor opcional. Se preenchida, deve ser `#RRGGBB` (7 chars, `#` +
/// 6 hex). Retorna a string normalizada (lowercase).
fn validate_color(color: String) -> Result<String, CoreError> {
    let c = color.trim().to_lowercase();
    if c.is_empty() {
        return Ok(String::new());
    }
    let ok = c.len() == 7
        && c.starts_with('#')
        && c[1..].chars().all(|ch| ch.is_ascii_hexdigit());
    if !ok {
        return Err(CoreError::Validation(
            "Cor deve estar no formato #RRGGBB".into(),
        ));
    }
    Ok(c)
}
