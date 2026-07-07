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

use uuid::Uuid;

use letaf_core::error::CoreError;
use letaf_core::reconcile::{ManifestEntry, ReconcileRepository, RECONCILE_TABLES};

use super::SyncWorker;

impl SyncWorker {
    /// Reconcilia todas as entidades. Se houver divergência servidor→local em
    /// qualquer uma, reseta o cursor de pull para forçar re-pull completo.
    pub(super) async fn reconcile_all(&self, token: &str) {
        let cid = self.state.company_id();
        let mut server_drift = false;
        for &table in RECONCILE_TABLES {
            match self.reconcile_entity(token, cid, table).await {
                Ok(drift) => server_drift |= drift,
                Err(e) => tracing::warn!("Reconcile {table}: {e}"),
            }
        }
        if server_drift {
            tracing::info!(
                "Reconcile: divergência servidor→local detectada; forçando re-pull completo neste ciclo"
            );
            // Reset do cursor em memória → o `pull_all` deste ciclo lê
            // `since = época` e re-puxa tudo (LWW idempotente). Ao concluir,
            // o cursor volta a avançar e é persistido. Não persistimos a
            // época aqui: se o re-pull falhar, o próximo reconcile re-detecta.
            match self.last_pull_at.lock() {
                Ok(mut g) => *g = None,
                Err(p) => *p.into_inner() = None,
            }
        }
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
