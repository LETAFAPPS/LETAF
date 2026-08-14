use async_trait::async_trait;
use chrono::NaiveDateTime;
use uuid::Uuid;

use super::model::Customer;
use crate::error::CoreError;

/// Trait de acesso a dados para Customer.
///
/// Regras aplicadas (AI_RULES.md §10):
/// - Acesso ao banco somente via repository
/// - Usar traits para abstração
///
/// Cada implementação concreta (PostgreSQL, SQLite) ficará
/// na camada correspondente (server/repository, desktop/repository).
#[async_trait]
pub trait CustomerRepository: Send + Sync {
    async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Customer>, CoreError>;
    async fn find_all(&self, company_id: Uuid) -> Result<Vec<Customer>, CoreError>;

    /// Conta os registros ATIVOS da empresa (para o painel do super admin).
    ///
    /// Implementação padrão carrega a lista — suficiente para o SQLite
    /// local, que é pequeno. O PostgreSQL sobrescreve com `COUNT(*)` para
    /// não trazer blobs/linhas inteiras só para contar (§13).
    async fn count_all(&self, company_id: Uuid) -> Result<i64, CoreError> {
        Ok(self.find_all(company_id).await?.len() as i64)
    }

    async fn find_by_email(&self, company_id: Uuid, email: &str) -> Result<Option<Customer>, CoreError>;

    /// Busca por TELEFONE (só dígitos) — usado no login por telefone do
    /// cliente final. Implementação padrão filtra `find_all` (suficiente para
    /// o SQLite local, pequeno; o login de cliente é server-only). O
    /// PostgreSQL sobrescreve com query direta (§13). Compara dígito a dígito
    /// dos dois lados, tolerando formatação divergente no armazenado.
    async fn find_by_phone(&self, company_id: Uuid, phone_digits: &str) -> Result<Option<Customer>, CoreError> {
        let alvo: String = phone_digits.chars().filter(char::is_ascii_digit).collect();
        if alvo.is_empty() {
            return Ok(None);
        }
        Ok(self.find_all(company_id).await?.into_iter().find(|c| {
            c.phone
                .as_deref()
                .map(|p| p.chars().filter(char::is_ascii_digit).collect::<String>())
                .as_deref()
                == Some(alvo.as_str())
        }))
    }

    async fn create(&self, customer: &Customer) -> Result<(), CoreError>;
    async fn update(&self, customer: &Customer) -> Result<(), CoreError>;
    async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError>;
    async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Customer>, CoreError>;
    async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError>;

    /// Upsert de sincronização (§7.7 — last-write-wins via updated_at).
    async fn sync_upsert(&self, customer: &Customer) -> Result<(), CoreError>;

    /// Versão de credencial do cliente, para revogação de sessão web (§11).
    /// `None` = cliente inexistente ou banido (`deleted_at`) → sessão inválida;
    /// `Some(v)` deve casar com o `tv` do JWT. É AUTORIDADE DO SERVIDOR: não
    /// entra no struct `Customer` nem no sync. Default (SQLite/offline) devolve
    /// `None` e nunca é chamado — auth de cliente só existe no servidor.
    async fn find_token_version(&self, _company_id: Uuid, _id: Uuid) -> Result<Option<i32>, CoreError> {
        Ok(None)
    }

    /// Incrementa a versão de credencial do cliente (revoga tokens ativos).
    /// Default no-op — offline não autentica cliente final.
    async fn bump_token_version(&self, _company_id: Uuid, _id: Uuid) -> Result<(), CoreError> {
        Ok(())
    }

    /// Busca entidades atualizadas após o timestamp (§7 — sync pull).
    async fn find_updated_since(&self, company_id: Uuid, since: NaiveDateTime) -> Result<Vec<Customer>, CoreError>;

    /// Página do pull por keyset `(updated_at, id)` — ver
    /// `ProductRepository::find_updated_since_paged`. Default delega ao
    /// não-paginado; só o Postgres sobrescreve.
    async fn find_updated_since_paged(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
        _after_id: Uuid,
        _limit: i64,
    ) -> Result<Vec<Customer>, CoreError> {
        self.find_updated_since(company_id, since).await
    }
}
