//! Reconciliação entre bancos (anti-entropia) — AI_RULES §7.
//!
//! O sync incremental (push `synced=false` + pull por cursor `updated_at >
//! since`) é rápido, mas pode deixar LACUNAS permanentes: um registro cujo
//! `updated_at` fica ABAIXO do cursor de um computador (relógio atrasado,
//! escrita fora de ordem, ou registro marcado `synced` que sumiu do servidor)
//! nunca mais é reconferido. A reconciliação fecha isso: compara o CONJUNTO
//! completo `(id, updated_at, deleted_at)` de cada entidade entre os dois
//! bancos e sincroniza o que estiver divergente ou faltando — nos dois
//! sentidos —, independente do cursor e do flag `synced`.
//!
//! Transporte barato: só ids + timestamps trafegam; o reparo reaproveita o
//! push/pull existentes (LWW idempotente §7.7). Multi-tenant: toda consulta
//! filtra por `company_id` (§11).

use async_trait::async_trait;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::CoreError;

/// Uma linha no manifesto de uma entidade: identidade + versão + tombstone.
/// `deleted_at` viaja para que soft-deletes também reconciliem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub id: Uuid,
    pub updated_at: NaiveDateTime,
    #[serde(default)]
    pub deleted_at: Option<NaiveDateTime>,
}

/// Tabelas de tenant reconciliáveis (todas com `company_id`, `updated_at`,
/// `deleted_at`, `synced` e cobertas pelo pull incremental). Fonte da verdade
/// da allowlist — nomes de tabela NUNCA vêm do cliente sem passar por aqui
/// (defesa contra SQL injection na query genérica de manifesto).
///
/// `companies` entra com filtro por `id` (é o próprio tenant, sem coluna
/// `company_id` — ver [`tenant_key_column`]). Exclusões intencionais: `plans`
/// (catálogo global, não sincroniza por tenant), `order_items` (filho do
/// agregado Order — reconcilia junto do pedido), `payment_charges` (criadas no
/// servidor), tabelas de junção, `stock_movements` (ledger append-only com
/// sync próprio, sem pull incremental — o reparo servidor→local não se aplica)
/// e `business_hours`: o manifesto compara por `id`, mas o upsert de horários
/// resolve conflito pela chave NATURAL `(company_id, day_of_week)` mantendo o
/// `id` local. Ids distintos para o mesmo dia (criado em origens diferentes)
/// fariam a anti-entropia por `id` acusar divergência a cada ciclo (ping-pong).
/// Horários convergem pelo sync incremental + LWW sobre a chave natural.
pub const RECONCILE_TABLES: &[&str] = &[
    "companies",
    "products",
    "categories",
    "subcategories",
    "customers",
    "customer_addresses",
    "addon_groups",
    "addons",
    "banners",
    "coupons",
    "orders",
    "cash_sessions",
    "cash_movements",
    "finance_categories",
    "finance_entries",
    "wallet_accounts",
    "wallet_movements",
    "subscriptions",
    "subscription_invoices",
    "payment_methods",
    "job_roles",
    "users",
];

/// `true` se `table` é uma entidade reconciliável conhecida. Chamado antes de
/// qualquer query que interpole o nome da tabela.
pub fn is_reconcilable(table: &str) -> bool {
    RECONCILE_TABLES.contains(&table)
}

/// Coluna de isolamento do tenant para a query de manifesto. Quase toda
/// entidade filtra por `company_id`; `companies` é o próprio tenant (a chave é
/// o `id`). Retorno é uma const (nunca vem do cliente) — seguro interpolar.
pub fn tenant_key_column(table: &str) -> &'static str {
    if table == "companies" {
        "id"
    } else {
        "company_id"
    }
}

/// Resultado da comparação entre o manifesto local e o do servidor.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ManifestDiff {
    /// O servidor tem registros ausentes no local OU com `updated_at` mais
    /// novo → reparo servidor→local (re-pull).
    pub server_drift: bool,
    /// Ids que só existem no local OU têm `updated_at` mais novo que o
    /// servidor → reparo local→servidor (marcar `synced=false` p/ re-push).
    pub push_ids: Vec<Uuid>,
}

/// Compara dois manifestos por `id`/`updated_at` (last-write-wins, §7.7).
/// Função PURA — testável sem rede/banco. `deleted_at` não muda a decisão: um
/// soft-delete tem `updated_at` mais recente, então já é tratado como "mais
/// novo" e propagado pelo lado vencedor.
pub fn diff(local: &[ManifestEntry], server: &[ManifestEntry]) -> ManifestDiff {
    use std::collections::HashMap;
    let local_map: HashMap<Uuid, NaiveDateTime> =
        local.iter().map(|e| (e.id, e.updated_at)).collect();
    let server_map: HashMap<Uuid, NaiveDateTime> =
        server.iter().map(|e| (e.id, e.updated_at)).collect();

    let server_drift = server.iter().any(|s| match local_map.get(&s.id) {
        None => true,
        Some(&l) => s.updated_at > l,
    });
    let push_ids = local
        .iter()
        .filter(|l| match server_map.get(&l.id) {
            None => true,
            Some(&s) => l.updated_at > s,
        })
        .map(|l| l.id)
        .collect();

    ManifestDiff { server_drift, push_ids }
}

/// Acesso genérico ao manifesto de uma entidade. Uma implementação por banco
/// (Postgres no servidor, SQLite no desktop). O nome da tabela é validado
/// contra [`RECONCILE_TABLES`] pela implementação antes de usar.
#[async_trait]
pub trait ReconcileRepository: Send + Sync {
    /// Manifesto completo da entidade para a empresa: `(id, updated_at,
    /// deleted_at)` de TODAS as linhas (inclusive soft-deletadas).
    async fn manifest(
        &self,
        company_id: Uuid,
        table: &str,
    ) -> Result<Vec<ManifestEntry>, CoreError>;

    /// Marca registros como não-sincronizados (`synced = false`), para que o
    /// push do próximo ciclo os reenvie — reparo do sentido local→servidor.
    async fn mark_unsynced(
        &self,
        company_id: Uuid,
        table: &str,
        ids: &[Uuid],
    ) -> Result<(), CoreError>;
}
