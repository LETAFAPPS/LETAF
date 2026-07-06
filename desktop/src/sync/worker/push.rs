//! Push de entidades pendentes (synced=false) ao servidor.
//! `impl SyncWorker` separado por responsabilidade (AI_RULES §8).
//!
//! Marcação de `synced` CONDICIONAL ao `updated_at` (AI_RULES §7.6): após o
//! push HTTP, `mark_synced(company_id, id, updated_at)` só marca `synced=true`
//! se a linha ainda tiver o `updated_at` que foi enviado
//! (`WHERE company_id=? AND id=? AND updated_at=?`). Assim, se o operador
//! editar o MESMO registro enquanto o push está em voo, o `updated_at` já
//! mudou → 0 linhas afetadas → o registro fica `synced=false` e é reenviado no
//! próximo ciclo (evita perda silenciosa da versão nova).
//!
//! ✅ Aplicado: subsistema PADRÃO (produtos, pedidos, clientes, categorias,
//! subcategorias, adicionais, grupos, banners, cupons, financeiro, categorias
//! financeiras, cargos, usuários, formas de pagamento, endereços, horários).
//! ⏳ Pendente (mesmos moldes, próximos commits): caixa, carteira, assinaturas.
//! `company` fica de fora (registro único do tenant, race negligenciável).

use letaf_core::auth::model::SyncUserPayload;
use letaf_core::error::CoreError;

use super::SyncWorker;

impl SyncWorker {

    /// Sincroniza produtos pendentes com o servidor.
    pub(super) async fn sync_products(&self, token: &str) -> Result<(), CoreError> {
        let items = self.state.product_service
            .find_unsynced(self.state.company_id()).await?;

        for item in &items {
            if self.send_one(token, "/sync/products", item.base.id, item).await {
                if let Err(e) = self.state.product_service
                    .mark_synced(self.state.company_id(), item.base.id, item.base.updated_at).await {
                    tracing::warn!("mark_synced product {}: {e}", item.base.id);
                }
            }
        }
        Ok(())
    }

    /// Sincroniza usuários pendentes com o servidor.
    ///
    /// Usa SyncUserPayload para incluir password_hash na serialização
    /// (User.password_hash tem skip_serializing por segurança em APIs).
    pub(super) async fn sync_job_roles(&self, token: &str) -> Result<(), CoreError> {
        let items = self.state.job_role_service
            .find_unsynced(self.state.company_id()).await?;

        for item in &items {
            if self.send_one(token, "/sync/job-roles", item.base.id, item).await {
                if let Err(e) = self.state.job_role_service
                    .mark_synced(self.state.company_id(), item.base.id, item.base.updated_at).await {
                    tracing::warn!("mark_synced job_role {}: {e}", item.base.id);
                }
            }
        }
        Ok(())
    }

    pub(super) async fn sync_users(&self, token: &str) -> Result<(), CoreError> {
        let items = self.state.auth_service
            .find_unsynced(self.state.company_id()).await?;

        for item in &items {
            let payload = SyncUserPayload::from(item);
            if self.send_one(token, "/sync/users", item.base.id, &payload).await {
                if let Err(e) = self.state.auth_service
                    .mark_synced(self.state.company_id(), item.base.id, item.base.updated_at).await {
                    tracing::warn!("mark_synced user {}: {e}", item.base.id);
                }
            }
        }
        Ok(())
    }

    /// Sincroniza empresas pendentes com o servidor.
    pub(super) async fn sync_companies(&self, token: &str) -> Result<(), CoreError> {
        let items = self.state.company_service
            .find_unsynced().await?;

        for item in &items {
            if self.send_one(token, "/sync/companies", item.id, item).await {
                if let Err(e) = self.state.company_service.mark_synced(item.id).await {
                    tracing::warn!("mark_synced company {}: {e}", item.id);
                }
            }
        }
        Ok(())
    }

    /// Sincroniza clientes pendentes com o servidor.
    pub(super) async fn sync_customers(&self, token: &str) -> Result<(), CoreError> {
        let items = self.state.customer_service
            .find_unsynced(self.state.company_id()).await?;

        for item in &items {
            if self.send_one(token, "/sync/customers", item.base.id, item).await {
                if let Err(e) = self.state.customer_service
                    .mark_synced(self.state.company_id(), item.base.id, item.base.updated_at).await {
                    tracing::warn!("mark_synced customer {}: {e}", item.base.id);
                }
            }
        }
        Ok(())
    }

    /// Sincroniza categorias pendentes com o servidor.
    pub(super) async fn sync_categories(&self, token: &str) -> Result<(), CoreError> {
        let items = self.state.category_service
            .find_unsynced(self.state.company_id()).await?;

        for item in &items {
            if self.send_one(token, "/sync/categories", item.base.id, item).await {
                if let Err(e) = self.state.category_service
                    .mark_synced(self.state.company_id(), item.base.id, item.base.updated_at).await {
                    tracing::warn!("mark_synced category {}: {e}", item.base.id);
                }
            }
        }
        Ok(())
    }

    /// Sincroniza subcategorias pendentes com o servidor.
    pub(super) async fn sync_subcategories(&self, token: &str) -> Result<(), CoreError> {
        let items = self.state.subcategory_service
            .find_unsynced(self.state.company_id()).await?;

        for item in &items {
            if self.send_one(token, "/sync/subcategories", item.base.id, item).await {
                if let Err(e) = self.state.subcategory_service
                    .mark_synced(self.state.company_id(), item.base.id, item.base.updated_at).await {
                    tracing::warn!("mark_synced subcategory {}: {e}", item.base.id);
                }
            }
        }
        Ok(())
    }

    /// Sincroniza horários de funcionamento pendentes com o servidor.
    pub(super) async fn sync_business_hours(&self, token: &str) -> Result<(), CoreError> {
        let items = self.state.business_hours_service
            .find_unsynced(self.state.company_id()).await?;
        for item in &items {
            if self.send_one(token, "/sync/business-hours", item.base.id, item).await {
                if let Err(e) = self.state.business_hours_service
                    .mark_synced(self.state.company_id(), item.base.id, item.base.updated_at).await {
                    tracing::warn!("mark_synced business_hours {}: {e}", item.base.id);
                }
            }
        }
        Ok(())
    }

    /// Sincroniza pedidos pendentes com o servidor.
    ///
    /// Regras aplicadas (AI_RULES.md §7.3, §7.5):
    /// - Envia pedidos com `synced = false` (mudanças locais de status, etc.)
    /// - Order serializa com seus items inclusos (vide `Order::items`)
    pub(super) async fn sync_orders(&self, token: &str) -> Result<(), CoreError> {
        let items = self.state.order_service
            .find_unsynced(self.state.company_id()).await?;

        for item in &items {
            if self.send_one(token, "/sync/orders", item.base.id, item).await {
                if let Err(e) = self.state.order_service
                    .mark_synced(self.state.company_id(), item.base.id, item.base.updated_at).await {
                    tracing::warn!("mark_synced order {}: {e}", item.base.id);
                }
            }
        }
        Ok(())
    }

    // ── Adicionais (Fase 4) ───────────────────────────────────

    /// Push de grupos de adicionais pendentes.
    pub(super) async fn sync_addon_groups(&self, token: &str) -> Result<(), CoreError> {
        let items = self.state.addon_group_service
            .find_unsynced(self.state.company_id()).await?;
        for item in &items {
            if self.send_one(token, "/sync/addon-groups", item.base.id, item).await {
                if let Err(e) = self.state.addon_group_service
                    .mark_synced(self.state.company_id(), item.base.id, item.base.updated_at).await {
                    tracing::warn!("mark_synced addon_group {}: {e}", item.base.id);
                }
            }
        }
        Ok(())
    }

    /// Push de adicionais pendentes (depois dos grupos para respeitar FK).
    pub(super) async fn sync_addons(&self, token: &str) -> Result<(), CoreError> {
        let items = self.state.addon_service
            .find_unsynced(self.state.company_id()).await?;
        for item in &items {
            if self.send_one(token, "/sync/addons", item.base.id, item).await {
                if let Err(e) = self.state.addon_service
                    .mark_synced(self.state.company_id(), item.base.id, item.base.updated_at).await {
                    tracing::warn!("mark_synced addon {}: {e}", item.base.id);
                }
            }
        }
        Ok(())
    }

    // ── Banners (Fase 7) ───────────────────────────────────

    /// Push de banners pendentes (depois de products para a FK lógica
    /// `item_id` referenciar produtos que já existem no servidor).
    pub(super) async fn sync_banners(&self, token: &str) -> Result<(), CoreError> {
        let items = self.state.banner_service
            .find_unsynced(self.state.company_id()).await?;
        for item in &items {
            if self.send_one(token, "/sync/banners", item.base.id, item).await {
                if let Err(e) = self.state.banner_service
                    .mark_synced(self.state.company_id(), item.base.id, item.base.updated_at).await {
                    tracing::warn!("mark_synced banner {}: {e}", item.base.id);
                }
            }
        }
        Ok(())
    }

    // ── Cupons (Fase 8) ────────────────────────────────────

    /// Push de cupons pendentes.
    pub(super) async fn sync_coupons(&self, token: &str) -> Result<(), CoreError> {
        let items = self.state.coupon_service
            .find_unsynced(self.state.company_id()).await?;
        for item in &items {
            if self.send_one(token, "/sync/coupons", item.base.id, item).await {
                if let Err(e) = self.state.coupon_service
                    .mark_synced(self.state.company_id(), item.base.id, item.base.updated_at).await {
                    tracing::warn!("mark_synced coupon {}: {e}", item.base.id);
                }
            }
        }
        Ok(())
    }

    // ── Endereços de cliente (Fase 9) ──────────────────────

    /// Push de endereços pendentes (depois de customers pela FK).
    pub(super) async fn sync_customer_addresses(&self, token: &str) -> Result<(), CoreError> {
        let items = self.state.customer_address_service
            .find_unsynced(self.state.company_id()).await?;
        for item in &items {
            if self.send_one(token, "/sync/customer-addresses", item.base.id, item).await {
                if let Err(e) = self.state.customer_address_service
                    .mark_synced(self.state.company_id(), item.base.id, item.base.updated_at).await {
                    tracing::warn!("mark_synced customer_address {}: {e}", item.base.id);
                }
            }
        }
        Ok(())
    }

    // ── Caixa (sessões + movimentos) ──────────────────────

    /// Push de sessões de caixa pendentes.
    pub(super) async fn sync_cash_sessions(&self, token: &str) -> Result<(), CoreError> {
        let cid = self.state.company_id();
        let items = self.state.cash_service.find_unsynced_sessions(cid).await?;
        for item in &items {
            if self.send_one(token, "/sync/cash-sessions", item.base.id, item).await {
                if let Err(e) = self.state.cash_service.mark_session_synced(cid, item.base.id).await {
                    tracing::warn!("mark_synced cash_session {}: {e}", item.base.id);
                }
            }
        }
        Ok(())
    }

    /// Push de movimentos pendentes (depois das sessões pela FK lógica).
    pub(super) async fn sync_cash_movements(&self, token: &str) -> Result<(), CoreError> {
        let cid = self.state.company_id();
        let items = self.state.cash_service.find_unsynced_movements(cid).await?;
        for item in &items {
            if self.send_one(token, "/sync/cash-movements", item.base.id, item).await {
                if let Err(e) = self.state.cash_service.mark_movement_synced(cid, item.base.id).await {
                    tracing::warn!("mark_synced cash_movement {}: {e}", item.base.id);
                }
            }
        }
        Ok(())
    }

    /// Push de categorias financeiras pendentes.
    pub(super) async fn sync_finance_categories(&self, token: &str) -> Result<(), CoreError> {
        let cid = self.state.company_id();
        let items = self.state.finance_category_service.find_unsynced(cid).await?;
        for item in &items {
            if self.send_one(token, "/sync/finance-categories", item.base.id, item).await {
                if let Err(e) = self.state.finance_category_service
                    .mark_synced(cid, item.base.id, item.base.updated_at).await
                {
                    tracing::warn!("mark_synced finance_category {}: {e}", item.base.id);
                }
            }
        }
        Ok(())
    }

    /// Push de lançamentos pendentes (depois das categorias pela FK lógica).
    pub(super) async fn sync_finance_entries(&self, token: &str) -> Result<(), CoreError> {
        let cid = self.state.company_id();
        let items = self.state.finance_service.find_unsynced(cid).await?;
        for item in &items {
            if self.send_one(token, "/sync/finance-entries", item.base.id, item).await {
                if let Err(e) = self.state.finance_service.mark_synced(cid, item.base.id, item.base.updated_at).await {
                    tracing::warn!("mark_synced finance_entry {}: {e}", item.base.id);
                }
            }
        }
        Ok(())
    }

    /// Push de contas-carteira pendentes.
    pub(super) async fn sync_wallet_accounts(&self, token: &str) -> Result<(), CoreError> {
        let cid = self.state.company_id();
        let items = self.state.wallet_service.find_unsynced_accounts(cid).await?;
        for item in &items {
            if self.send_one(token, "/sync/wallet-accounts", item.base.id, item).await {
                if let Err(e) = self.state.wallet_service
                    .mark_account_synced(cid, item.base.id).await
                {
                    tracing::warn!("mark_synced wallet_account {}: {e}", item.base.id);
                }
            }
        }
        Ok(())
    }

    /// Push de movimentos pendentes (depois das contas pela FK lógica).
    pub(super) async fn sync_wallet_movements(&self, token: &str) -> Result<(), CoreError> {
        let cid = self.state.company_id();
        let items = self.state.wallet_service.find_unsynced_movements(cid).await?;
        for item in &items {
            if self.send_one(token, "/sync/wallet-movements", item.base.id, item).await {
                if let Err(e) = self.state.wallet_service
                    .mark_movement_synced(cid, item.base.id).await
                {
                    tracing::warn!("mark_synced wallet_movement {}: {e}", item.base.id);
                }
            }
        }
        Ok(())
    }

    pub(super) async fn sync_subscriptions(&self, token: &str) -> Result<(), CoreError> {
        let items = self
            .state
            .subscription_service
            .find_unsynced_subscriptions(self.state.company_id())
            .await?;
        for item in &items {
            if self.send_one(token, "/sync/subscriptions", item.base.id, item).await {
                if let Err(e) = self
                    .state
                    .subscription_service
                    .mark_subscription_synced(self.state.company_id(), item.base.id)
                    .await
                {
                    tracing::warn!("mark_synced subscription {}: {e}", item.base.id);
                }
            }
        }
        Ok(())
    }

    pub(super) async fn sync_subscription_invoices(&self, token: &str) -> Result<(), CoreError> {
        let items = self
            .state
            .subscription_service
            .find_unsynced_invoices(self.state.company_id())
            .await?;
        for item in &items {
            if self
                .send_one(token, "/sync/subscription-invoices", item.base.id, item)
                .await
            {
                if let Err(e) = self
                    .state
                    .subscription_service
                    .mark_invoice_synced(self.state.company_id(), item.base.id)
                    .await
                {
                    tracing::warn!("mark_synced subscription_invoice {}: {e}", item.base.id);
                }
            }
        }
        Ok(())
    }

    pub(super) async fn sync_payment_methods(&self, token: &str) -> Result<(), CoreError> {
        let items = self
            .state
            .payment_method_service
            .find_unsynced(self.state.company_id())
            .await?;
        for item in &items {
            if self
                .send_one(token, "/sync/payment-methods", item.base.id, item)
                .await
            {
                if let Err(e) = self
                    .state
                    .payment_method_service
                    .mark_synced(self.state.company_id(), item.base.id, item.base.updated_at)
                    .await
                {
                    tracing::warn!("mark_synced payment_method {}: {e}", item.base.id);
                }
            }
        }
        Ok(())
    }
}

