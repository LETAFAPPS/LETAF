//! Pull de entidades atualizadas do servidor (last-write-wins).
//! `impl SyncWorker` separado por responsabilidade (AI_RULES §8).

use chrono::NaiveDateTime;

use letaf_core::addon::model::Addon;
use letaf_core::addon_group::model::AddonGroup;
use letaf_core::auth::model::SyncUserPayload;
use letaf_core::banner::model::Banner;
use letaf_core::payment_method::model::PaymentMethod;
use letaf_core::subscription::model::{Invoice as SubscriptionInvoice, Subscription};
use letaf_core::cash::model::{CashMovement, CashSession};
use letaf_core::finance::model::FinanceEntry;
use letaf_core::finance_category::model::FinanceCategory;
use letaf_core::wallet::model::{WalletAccount, WalletMovement};
use letaf_core::coupon::model::Coupon;
use letaf_core::customer_address::model::CustomerAddress;
use letaf_core::business_hours::model::BusinessHours;
use letaf_core::category::model::Category;
use letaf_core::job_role::model::JobRole;
use letaf_core::subcategory::model::Subcategory;
use letaf_core::company::model::Company;
use letaf_core::customer::model::Customer;
use letaf_core::order::model::Order;
use letaf_core::product::model::Product;
use letaf_core::error::CoreError;

use super::{PullCursor, SyncWorker};

// Cursor keyset para as entidades grandes (pull paginado, §7/§13).
impl PullCursor for Product {
    fn pull_cursor(&self) -> (NaiveDateTime, uuid::Uuid) { (self.base.updated_at, self.base.id) }
}
impl PullCursor for Customer {
    fn pull_cursor(&self) -> (NaiveDateTime, uuid::Uuid) { (self.base.updated_at, self.base.id) }
}
impl PullCursor for FinanceEntry {
    fn pull_cursor(&self) -> (NaiveDateTime, uuid::Uuid) { (self.base.updated_at, self.base.id) }
}
impl PullCursor for CashMovement {
    fn pull_cursor(&self) -> (NaiveDateTime, uuid::Uuid) { (self.base.updated_at, self.base.id) }
}
impl PullCursor for WalletMovement {
    fn pull_cursor(&self) -> (NaiveDateTime, uuid::Uuid) { (self.base.updated_at, self.base.id) }
}
impl PullCursor for Order {
    fn pull_cursor(&self) -> (NaiveDateTime, uuid::Uuid) { (self.base.updated_at, self.base.id) }
}
impl PullCursor for CustomerAddress {
    fn pull_cursor(&self) -> (NaiveDateTime, uuid::Uuid) { (self.base.updated_at, self.base.id) }
}
impl PullCursor for CashSession {
    fn pull_cursor(&self) -> (NaiveDateTime, uuid::Uuid) { (self.base.updated_at, self.base.id) }
}
impl PullCursor for SubscriptionInvoice {
    fn pull_cursor(&self) -> (NaiveDateTime, uuid::Uuid) { (self.base.updated_at, self.base.id) }
}
impl PullCursor for WalletAccount {
    fn pull_cursor(&self) -> (NaiveDateTime, uuid::Uuid) { (self.base.updated_at, self.base.id) }
}

impl SyncWorker {

    /// Pull de horários de funcionamento do servidor.
    pub(super) async fn pull_business_hours(&self, token: &str, since: NaiveDateTime, mut max_ts: NaiveDateTime) -> Result<NaiveDateTime, CoreError> {
        let items: Vec<BusinessHours> = self.fetch_pull(token, "/sync/pull/business-hours", since).await?;
        let cid = self.state.company_id();
        for item in items {
            if item.base.updated_at > max_ts { max_ts = item.base.updated_at; }
            self.state.business_hours_service.sync_upsert(cid, item).await?;
        }
        Ok(max_ts)
    }

    /// Pull de produtos do servidor (paginado — base pode ser grande).
    pub(super) async fn pull_products(&self, token: &str, since: NaiveDateTime, mut max_ts: NaiveDateTime) -> Result<NaiveDateTime, CoreError> {
        let items: Vec<Product> = self.fetch_pull_paged(token, "/sync/pull/products", since).await?;
        let cid = self.state.company_id();
        for item in items {
            if item.base.updated_at > max_ts { max_ts = item.base.updated_at; }
            self.state.product_service.sync_upsert(cid, item).await?;
        }
        Ok(max_ts)
    }

    /// Pull de usuários do servidor.
    pub(super) async fn pull_users(&self, token: &str, since: NaiveDateTime, mut max_ts: NaiveDateTime) -> Result<NaiveDateTime, CoreError> {
        let items: Vec<SyncUserPayload> = self.fetch_pull(token, "/sync/pull/users", since).await?;
        let cid = self.state.company_id();
        for item in items {
            if item.updated_at > max_ts { max_ts = item.updated_at; }
            self.state.auth_service.sync_upsert(cid, item).await?;
        }
        Ok(max_ts)
    }

    /// Pull de clientes do servidor.
    pub(super) async fn pull_customers(&self, token: &str, since: NaiveDateTime, mut max_ts: NaiveDateTime) -> Result<NaiveDateTime, CoreError> {
        let items: Vec<Customer> = self.fetch_pull_paged(token, "/sync/pull/customers", since).await?;
        let cid = self.state.company_id();
        for item in items {
            if item.base.updated_at > max_ts { max_ts = item.base.updated_at; }
            self.state.customer_service.sync_upsert(cid, item).await?;
        }
        Ok(max_ts)
    }

    /// Pull de categorias do servidor.
    pub(super) async fn pull_categories(&self, token: &str, since: NaiveDateTime, mut max_ts: NaiveDateTime) -> Result<NaiveDateTime, CoreError> {
        let items: Vec<Category> = self.fetch_pull(token, "/sync/pull/categories", since).await?;
        let cid = self.state.company_id();
        for item in items {
            if item.base.updated_at > max_ts { max_ts = item.base.updated_at; }
            self.state.category_service.sync_upsert(cid, item).await?;
        }
        Ok(max_ts)
    }

    pub(super) async fn pull_job_roles(&self, token: &str, since: NaiveDateTime, mut max_ts: NaiveDateTime) -> Result<NaiveDateTime, CoreError> {
        let items: Vec<JobRole> = self.fetch_pull(token, "/sync/pull/job-roles", since).await?;
        let cid = self.state.company_id();
        for item in items {
            if item.base.updated_at > max_ts { max_ts = item.base.updated_at; }
            self.state.job_role_service.sync_upsert(cid, item).await?;
        }
        Ok(max_ts)
    }

    /// Pull de subcategorias do servidor.
    pub(super) async fn pull_subcategories(&self, token: &str, since: NaiveDateTime, mut max_ts: NaiveDateTime) -> Result<NaiveDateTime, CoreError> {
        let items: Vec<Subcategory> = self.fetch_pull(token, "/sync/pull/subcategories", since).await?;
        let cid = self.state.company_id();
        for item in items {
            if item.base.updated_at > max_ts { max_ts = item.base.updated_at; }
            self.state.subcategory_service.sync_upsert(cid, item).await?;
        }
        Ok(max_ts)
    }

    /// Pull de empresas do servidor.
    pub(super) async fn pull_companies(&self, token: &str, since: NaiveDateTime, mut max_ts: NaiveDateTime) -> Result<NaiveDateTime, CoreError> {
        let items: Vec<Company> = self.fetch_pull(token, "/sync/pull/companies", since).await?;
        let cid = self.state.company_id();
        for item in items {
            if item.updated_at > max_ts { max_ts = item.updated_at; }
            self.state.company_service.sync_upsert(cid, item).await?;
        }
        Ok(max_ts)
    }

    /// Pull de pedidos do servidor.
    ///
    /// Regras aplicadas (AI_RULES.md §7.7):
    /// - Upsert via service (last-write-wins via `updated_at`)
    /// - Inclui itens serializados junto do pedido
    ///
    /// Para cada pedido puxado, consulta o `AlarmWatcher` — se o ID é
    /// inédito na sessão E o status é `Pending`, dispara o alarme:
    /// (1) inicia o `AlarmPlayer` (loop de beeps em thread dedicada),
    /// (2) sinaliza `alarm_signal` para a UI abrir o modal.
    /// Como o watcher é função pura e o player é idempotente, chamar
    /// múltiplas vezes em rajada (vários pedidos novos no mesmo pull)
    /// é seguro: o som não duplica e o modal só abre uma vez.
    pub(super) async fn pull_orders(&self, token: &str, since: NaiveDateTime, mut max_ts: NaiveDateTime) -> Result<NaiveDateTime, CoreError> {
        let items: Vec<Order> = self.fetch_pull_paged(token, "/sync/pull/orders", since).await?;
        let cid = self.state.company_id();
        let mut any_new = false;
        for item in items {
            if item.base.updated_at > max_ts { max_ts = item.base.updated_at; }
            // `note` antes do upsert (que move `item`) — evita clonar
            // cada pedido por pull.
            if self.state.alarm_watcher.note(&item) {
                any_new = true;
            }
            self.state.order_service.sync_upsert(cid, item).await?;
        }
        if any_new {
            self.state.alarm_player.start();
            self.state.alarm_signal.notify_one();
        }
        Ok(max_ts)
    }

    pub(super) async fn pull_addon_groups(&self, token: &str, since: NaiveDateTime, mut max_ts: NaiveDateTime) -> Result<NaiveDateTime, CoreError> {
        let items: Vec<AddonGroup> = self.fetch_pull(token, "/sync/pull/addon-groups", since).await?;
        let cid = self.state.company_id();
        for item in items {
            if item.base.updated_at > max_ts { max_ts = item.base.updated_at; }
            self.state.addon_group_service.sync_upsert(cid, item).await?;
        }
        Ok(max_ts)
    }

    pub(super) async fn pull_addons(&self, token: &str, since: NaiveDateTime, mut max_ts: NaiveDateTime) -> Result<NaiveDateTime, CoreError> {
        let items: Vec<Addon> = self.fetch_pull(token, "/sync/pull/addons", since).await?;
        let cid = self.state.company_id();
        for item in items {
            if item.base.updated_at > max_ts { max_ts = item.base.updated_at; }
            self.state.addon_service.sync_upsert(cid, item).await?;
        }
        Ok(max_ts)
    }

    pub(super) async fn pull_banners(&self, token: &str, since: NaiveDateTime, mut max_ts: NaiveDateTime) -> Result<NaiveDateTime, CoreError> {
        let items: Vec<Banner> = self.fetch_pull(token, "/sync/pull/banners", since).await?;
        let cid = self.state.company_id();
        for item in items {
            if item.base.updated_at > max_ts { max_ts = item.base.updated_at; }
            self.state.banner_service.sync_upsert(cid, item).await?;
        }
        Ok(max_ts)
    }

    pub(super) async fn pull_coupons(&self, token: &str, since: NaiveDateTime, mut max_ts: NaiveDateTime) -> Result<NaiveDateTime, CoreError> {
        let items: Vec<Coupon> = self.fetch_pull(token, "/sync/pull/coupons", since).await?;
        let cid = self.state.company_id();
        for item in items {
            if item.base.updated_at > max_ts { max_ts = item.base.updated_at; }
            self.state.coupon_service.sync_upsert(cid, item).await?;
        }
        Ok(max_ts)
    }

    pub(super) async fn pull_customer_addresses(&self, token: &str, since: NaiveDateTime, mut max_ts: NaiveDateTime) -> Result<NaiveDateTime, CoreError> {
        let items: Vec<CustomerAddress> = self.fetch_pull_paged(token, "/sync/pull/customer-addresses", since).await?;
        let cid = self.state.company_id();
        for item in items {
            if item.base.updated_at > max_ts { max_ts = item.base.updated_at; }
            self.state.customer_address_service.sync_upsert(cid, item).await?;
        }
        Ok(max_ts)
    }

    pub(super) async fn pull_cash_sessions(&self, token: &str, since: NaiveDateTime, mut max_ts: NaiveDateTime) -> Result<NaiveDateTime, CoreError> {
        let items: Vec<CashSession> = self.fetch_pull_paged(token, "/sync/pull/cash-sessions", since).await?;
        let cid = self.state.company_id();
        for item in items {
            if item.base.updated_at > max_ts { max_ts = item.base.updated_at; }
            self.state.cash_service.sync_upsert_session(cid, item).await?;
        }
        Ok(max_ts)
    }

    pub(super) async fn pull_cash_movements(&self, token: &str, since: NaiveDateTime, mut max_ts: NaiveDateTime) -> Result<NaiveDateTime, CoreError> {
        let items: Vec<CashMovement> = self.fetch_pull_paged(token, "/sync/pull/cash-movements", since).await?;
        let cid = self.state.company_id();
        for item in items {
            if item.base.updated_at > max_ts { max_ts = item.base.updated_at; }
            self.state.cash_service.sync_upsert_movement(cid, item).await?;
        }
        Ok(max_ts)
    }

    pub(super) async fn pull_finance_categories(
        &self, token: &str, since: NaiveDateTime, mut max_ts: NaiveDateTime,
    ) -> Result<NaiveDateTime, CoreError> {
        let items: Vec<FinanceCategory> =
            self.fetch_pull(token, "/sync/pull/finance-categories", since).await?;
        let cid = self.state.company_id();
        for item in items {
            if item.base.updated_at > max_ts { max_ts = item.base.updated_at; }
            self.state.finance_category_service.sync_upsert(cid, item).await?;
        }
        Ok(max_ts)
    }

    pub(super) async fn pull_finance_entries(
        &self, token: &str, since: NaiveDateTime, mut max_ts: NaiveDateTime,
    ) -> Result<NaiveDateTime, CoreError> {
        let items: Vec<FinanceEntry> =
            self.fetch_pull_paged(token, "/sync/pull/finance-entries", since).await?;
        let cid = self.state.company_id();
        for item in items {
            if item.base.updated_at > max_ts { max_ts = item.base.updated_at; }
            self.state.finance_service.sync_upsert(cid, item).await?;
        }
        Ok(max_ts)
    }

    pub(super) async fn pull_wallet_accounts(
        &self, token: &str, since: NaiveDateTime, mut max_ts: NaiveDateTime,
    ) -> Result<NaiveDateTime, CoreError> {
        let items: Vec<WalletAccount> =
            self.fetch_pull_paged(token, "/sync/pull/wallet-accounts", since).await?;
        let cid = self.state.company_id();
        for item in items {
            if item.base.updated_at > max_ts { max_ts = item.base.updated_at; }
            self.state.wallet_service.sync_upsert_account(cid, item).await?;
        }
        Ok(max_ts)
    }

    pub(super) async fn pull_wallet_movements(
        &self, token: &str, since: NaiveDateTime, mut max_ts: NaiveDateTime,
    ) -> Result<NaiveDateTime, CoreError> {
        let items: Vec<WalletMovement> =
            self.fetch_pull_paged(token, "/sync/pull/wallet-movements", since).await?;
        let cid = self.state.company_id();
        for item in items {
            if item.base.updated_at > max_ts { max_ts = item.base.updated_at; }
            self.state.wallet_service.sync_upsert_movement(cid, item).await?;
        }
        Ok(max_ts)
    }

    pub(super) async fn pull_subscriptions(
        &self,
        token: &str,
        since: NaiveDateTime,
        mut max_ts: NaiveDateTime,
    ) -> Result<NaiveDateTime, CoreError> {
        let items: Vec<Subscription> =
            self.fetch_pull(token, "/sync/pull/subscriptions", since).await?;
        let cid = self.state.company_id();
        for item in items {
            if item.base.updated_at > max_ts {
                max_ts = item.base.updated_at;
            }
            self.state
                .subscription_service
                .sync_upsert_subscription(cid, item)
                .await?;
        }
        Ok(max_ts)
    }

    pub(super) async fn pull_subscription_invoices(
        &self,
        token: &str,
        since: NaiveDateTime,
        mut max_ts: NaiveDateTime,
    ) -> Result<NaiveDateTime, CoreError> {
        let items: Vec<SubscriptionInvoice> = self
            .fetch_pull_paged(token, "/sync/pull/subscription-invoices", since)
            .await?;
        let cid = self.state.company_id();
        for item in items {
            if item.base.updated_at > max_ts {
                max_ts = item.base.updated_at;
            }
            self.state
                .subscription_service
                .sync_upsert_invoice(cid, item)
                .await?;
        }
        Ok(max_ts)
    }

    pub(super) async fn pull_payment_methods(
        &self,
        token: &str,
        since: NaiveDateTime,
        mut max_ts: NaiveDateTime,
    ) -> Result<NaiveDateTime, CoreError> {
        let items: Vec<PaymentMethod> =
            self.fetch_pull(token, "/sync/pull/payment-methods", since).await?;
        let cid = self.state.company_id();
        for item in items {
            if item.base.updated_at > max_ts {
                max_ts = item.base.updated_at;
            }
            self.state
                .payment_method_service
                .sync_upsert(cid, item)
                .await?;
        }
        Ok(max_ts)
    }
}

