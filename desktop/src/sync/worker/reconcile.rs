//! Reconciliação anti-entropia (AI_RULES §7) — rede de segurança sobre o sync
//! incremental. Compara o manifesto completo `(id, updated_at, deleted_at)` de
//! cada entidade entre o banco LOCAL e o SERVIDOR e repara divergências nos
//! dois sentidos, independente do cursor de pull e do flag `synced`:
//!
//! - **local→servidor** (falta no servidor OU mais novo no local): marca os
//!   registros `synced=false` → o push do ciclo reenvia.
//! - **servidor→local** (falta no local OU mais novo no servidor): reseta o
//!   cursor de pull → o `pull_all` do ciclo re-puxa tudo (upsert LWW
//!   idempotente §7.7) e converge.
//!
//! Assim, um registro cujo `updated_at` ficou ABAIXO do cursor (relógio
//! atrasado, escrita fora de ordem, `synced` marcado mas ausente no servidor)
//! deixa de ficar preso — é reconferido e sincronizado.

use chrono::NaiveDateTime;
use uuid::Uuid;

use letaf_core::error::CoreError;
use letaf_core::reconcile::{ManifestEntry, ReconcileRepository, RECONCILE_TABLES};

use super::SyncWorker;

impl SyncWorker {
    /// Reconcilia todas as entidades. Para cada uma com divergência
    /// servidor→local, re-puxa APENAS aquela entidade (desde a época; upsert
    /// LWW idempotente) — sem perturbar o cursor incremental global.
    pub(super) async fn reconcile_all(&self, token: &str) {
        let cid = self.state.company_id();
        let mut drifted: Vec<&'static str> = Vec::new();
        for &table in RECONCILE_TABLES {
            match self.reconcile_entity(token, cid, table).await {
                Ok(true) => drifted.push(table),
                Ok(false) => {}
                Err(e) => tracing::warn!("Reconcile {table}: {e}"),
            }
        }
        if !drifted.is_empty() {
            tracing::info!("Reconcile: re-puxando entidades divergentes: {drifted:?}");
            for table in drifted {
                if let Err(e) = self.repull_entity(token, table).await {
                    tracing::warn!("Reconcile re-pull {table}: {e}");
                }
            }
        }
    }

    /// Re-puxa UMA entidade desde a época (traz tudo do servidor; upsert LWW
    /// idempotente). Ignora o `max_ts` retornado: o cursor incremental global
    /// não é tocado — este é um reparo pontual, não o pull do ciclo.
    async fn repull_entity(&self, token: &str, table: &str) -> Result<(), CoreError> {
        let epoch = NaiveDateTime::default();
        match table {
            "companies" => self.pull_companies(token, epoch, epoch).await?,
            "job_roles" => self.pull_job_roles(token, epoch, epoch).await?,
            "users" => self.pull_users(token, epoch, epoch).await?,
            "customers" => self.pull_customers(token, epoch, epoch).await?,
            "categories" => self.pull_categories(token, epoch, epoch).await?,
            "subcategories" => self.pull_subcategories(token, epoch, epoch).await?,
            "addon_groups" => self.pull_addon_groups(token, epoch, epoch).await?,
            "addons" => self.pull_addons(token, epoch, epoch).await?,
            "products" => self.pull_products(token, epoch, epoch).await?,
            "orders" => self.pull_orders(token, epoch, epoch).await?,
            "banners" => self.pull_banners(token, epoch, epoch).await?,
            "coupons" => self.pull_coupons(token, epoch, epoch).await?,
            "customer_addresses" => self.pull_customer_addresses(token, epoch, epoch).await?,
            "cash_sessions" => self.pull_cash_sessions(token, epoch, epoch).await?,
            "cash_movements" => self.pull_cash_movements(token, epoch, epoch).await?,
            "finance_categories" => self.pull_finance_categories(token, epoch, epoch).await?,
            "finance_entries" => self.pull_finance_entries(token, epoch, epoch).await?,
            "wallet_accounts" => self.pull_wallet_accounts(token, epoch, epoch).await?,
            "wallet_movements" => self.pull_wallet_movements(token, epoch, epoch).await?,
            "subscriptions" => self.pull_subscriptions(token, epoch, epoch).await?,
            "subscription_invoices" => self.pull_subscription_invoices(token, epoch, epoch).await?,
            "payment_methods" => self.pull_payment_methods(token, epoch, epoch).await?,
            other => {
                return Err(CoreError::Validation(format!(
                    "Reconcile: entidade sem pull dedicado: {other}"
                )))
            }
        };
        Ok(())
    }

    /// Reconcilia UMA entidade. Retorna `true` se o servidor tem registros
    /// ausentes/mais-novos no local (drift servidor→local). Os que só
    /// existem/estão mais novos no LOCAL são marcados `synced=false`.
    async fn reconcile_entity(
        &self,
        token: &str,
        cid: Uuid,
        table: &str,
    ) -> Result<bool, CoreError> {
        let server = self.fetch_manifest(token, table).await?;
        let local = self.state.reconcile.manifest(cid, table).await?;

        let d = letaf_core::reconcile::diff(&local, &server);
        if !d.push_ids.is_empty() {
            tracing::info!(
                "Reconcile {table}: {} registro(s) local→servidor a reenviar",
                d.push_ids.len()
            );
            self.state.reconcile.mark_unsynced(cid, table, &d.push_ids).await?;
        }
        Ok(d.server_drift)
    }

    /// Busca o manifesto de uma entidade no servidor (GET autenticado).
    async fn fetch_manifest(
        &self,
        token: &str,
        table: &str,
    ) -> Result<Vec<ManifestEntry>, CoreError> {
        let url = format!(
            "{}/sync/reconcile/manifest?entity={}",
            self.server_url, table
        );
        let resp = self
            .http
            .get(&url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| {
                self.flag_network_failure();
                CoreError::Repository(format!("Manifest {table}: {e}"))
            })?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            *self.auth_token.write().await = None;
            return Err(CoreError::Unauthorized(
                "JWT expirado durante reconcile".into(),
            ));
        }
        if !resp.status().is_success() {
            return Err(CoreError::Repository(format!(
                "Manifest {table}: status {}",
                resp.status()
            )));
        }
        resp.json::<Vec<ManifestEntry>>().await.map_err(|e| {
            CoreError::Repository(format!("Manifest {table} decode: {e}"))
        })
    }
}
