use async_trait::async_trait;
use chrono::NaiveDateTime;
use uuid::Uuid;

use super::model::{WalletAccount, WalletMovement};
use crate::error::CoreError;

/// Acesso a dados de carteiras de cliente.
///
/// Regras aplicadas (AI_RULES.md §4, §10):
/// - Todas as queries filtram por `company_id` (multi-tenant).
/// - `apply_movement` é a operação atômica chave: atualiza `balance`
///   da account e insere um `WalletMovement` em UMA transação só
///   (§4.Transações). Failures parciais inviabilizariam auditoria.
/// - Métodos de sync separados para account e movement porque o
///   pull do servidor pode chegar fora de ordem (last-write-wins via
///   `updated_at` em cada).
#[async_trait]
pub trait WalletRepository: Send + Sync {
    // ── Account ──

    async fn find_account_by_id(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<Option<WalletAccount>, CoreError>;

    /// Lookup pela chave natural (1:1 com customer). Service usa
    /// para garantir/criar account na primeira movimentação.
    async fn find_account_by_customer(
        &self,
        company_id: Uuid,
        customer_id: Uuid,
    ) -> Result<Option<WalletAccount>, CoreError>;

    async fn find_all_accounts(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<WalletAccount>, CoreError>;

    async fn create_account(&self, account: &WalletAccount) -> Result<(), CoreError>;
    async fn update_account(&self, account: &WalletAccount) -> Result<(), CoreError>;

    // ── Operação atômica balance + movement ──

    /// Aplica UM movimento e atualiza o balance da account em uma
    /// transação única. Service valida limite de fiado antes de
    /// chamar — esta camada confia no input.
    ///
    /// Marca tanto a account quanto o movimento como `synced = false`
    /// (escrita local pendente de sync — AI_RULES.md §7.3).
    async fn apply_movement(
        &self,
        account_new_state: &WalletAccount,
        movement: &WalletMovement,
    ) -> Result<(), CoreError>;

    // ── Movements (leitura) ──

    async fn find_movements_by_account(
        &self,
        company_id: Uuid,
        account_id: Uuid,
        limit: i64,
    ) -> Result<Vec<WalletMovement>, CoreError>;

    // ── Sync — accounts ──

    async fn find_unsynced_accounts(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<WalletAccount>, CoreError>;
    async fn mark_account_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError>;
    async fn find_accounts_updated_since(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
    ) -> Result<Vec<WalletAccount>, CoreError>;
    async fn sync_upsert_account(&self, account: &WalletAccount) -> Result<(), CoreError>;

    // ── Sync — movements ──

    async fn find_unsynced_movements(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<WalletMovement>, CoreError>;
    async fn mark_movement_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError>;
    async fn find_movements_updated_since(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
    ) -> Result<Vec<WalletMovement>, CoreError>;
    /// Página do pull de movimentos por keyset `(updated_at, id)` (default
    /// delega ao acima; só o Postgres sobrescreve).
    async fn find_movements_updated_since_paged(
        &self,
        company_id: Uuid,
        since: NaiveDateTime,
        _after_id: Uuid,
        _limit: i64,
    ) -> Result<Vec<WalletMovement>, CoreError> {
        self.find_movements_updated_since(company_id, since).await
    }
    async fn sync_upsert_movement(&self, movement: &WalletMovement) -> Result<(), CoreError>;
}
