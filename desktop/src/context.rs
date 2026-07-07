use std::sync::{Arc, RwLock};

use tokio::sync::Notify;
use uuid::Uuid;

use letaf_core::addon::service::AddonService;
use letaf_core::addon_group::service::AddonGroupService;
use letaf_core::auth::service::AuthService;
use letaf_core::banner::service::BannerService;
use letaf_core::business_hours::service::BusinessHoursService;
use letaf_core::cash::service::CashService;
use letaf_core::coupon::service::CouponService;
use letaf_core::category::service::CategoryService;
use letaf_core::job_role::service::JobRoleService;
use letaf_core::subcategory::service::SubcategoryService;
use letaf_core::subscription::service::SubscriptionService;
use letaf_core::payment_method::service::PaymentMethodService;
use letaf_core::company::service::CompanyService;
use letaf_core::customer::service::CustomerService;
use letaf_core::customer_address::service::CustomerAddressService;
use letaf_core::finance::service::FinanceService;
use letaf_core::finance_category::service::FinanceCategoryService;
use letaf_core::order::service::OrderService;
use letaf_core::wallet::service::WalletService;
use letaf_core::printer::service::PrinterService;
use letaf_core::product::service::ProductService;

use crate::alarm::{AlarmPlayer, AlarmWatcher};
use crate::session::SessionStore;
use crate::sync::status::SyncStatusHandle;

/// Estado da aplicação desktop.
///
/// Regras aplicadas (AI_RULES.md §1, §4, §7, §9):
/// - Desktop representa UMA empresa
/// - company_id atualizável via RwLock (após login pode mudar)
/// - Sistema funciona offline (SQLite)
/// - Services encapsulam repositories (inversão de dependência)
/// - `sync_status` é o canal leve worker → UI (§7)
#[derive(Clone)]
pub struct DesktopState {
    company_id: Arc<RwLock<Uuid>>,
    pub product_service: Arc<ProductService>,
    pub auth_service: Arc<AuthService>,
    pub company_service: Arc<CompanyService>,
    pub customer_service: Arc<CustomerService>,
    pub business_hours_service: Arc<BusinessHoursService>,
    pub category_service: Arc<CategoryService>,
    pub job_role_service: Arc<JobRoleService>,
    pub subcategory_service: Arc<SubcategoryService>,
    pub order_service: Arc<OrderService>,
    pub addon_group_service: Arc<AddonGroupService>,
    pub addon_service: Arc<AddonService>,
    pub banner_service: Arc<BannerService>,
    pub coupon_service: Arc<CouponService>,
    pub customer_address_service: Arc<CustomerAddressService>,
    /// Impressoras cadastradas localmente. Não sincroniza (per-device).
    pub printer_service: Arc<PrinterService>,
    /// Gestão de caixa (sessões + movimentos). Sincroniza com servidor.
    pub cash_service: Arc<CashService>,
    /// Categorias de lançamento financeiro (Fase 11). Sincroniza.
    pub finance_category_service: Arc<FinanceCategoryService>,
    /// Lançamentos a pagar/receber (Fase 11). Sincroniza.
    pub finance_service: Arc<FinanceService>,
    /// Carteira do cliente (Fase 12) — saldo + livro-razão.
    /// Sincroniza com servidor (multi-device).
    pub wallet_service: Arc<WalletService>,
    /// Assinatura/plano da empresa + histórico de faturas.
    /// O catálogo de planos vive no service (constantes) até o
    /// painel super-admin existir.
    pub subscription_service: Arc<SubscriptionService>,
    /// Catálogo de formas de pagamento cadastradas (Fase 14E).
    pub payment_method_service: Arc<PaymentMethodService>,
    /// Repositório genérico de reconciliação (anti-entropia — §7). Usado pelo
    /// SyncWorker para comparar manifestos local×servidor.
    pub reconcile: Arc<crate::repository::reconcile::SqliteReconcileRepository>,
    pub session: Arc<SessionStore>,
    pub sync_status: SyncStatusHandle,
    /// Alarme de novos pedidos — `watcher` decide se um pedido recém
    /// sincronizado deve disparar; `player` toca o beep em loop fora
    /// do event loop Slint (§1: UI sem lógica de negócio).
    pub alarm_watcher: Arc<AlarmWatcher>,
    pub alarm_player: Arc<AlarmPlayer>,
    /// `notify_one()` chamado pelo SyncWorker quando um pedido NOVO
    /// (status Pending, ID não visto) é puxado. Um task tokio do UI
    /// (`ui::alarm::setup_alarm_observer`) escuta `notified()` e
    /// invoca `slint::invoke_from_event_loop` para abrir o modal.
    pub alarm_signal: Arc<Notify>,
    /// Pausa o SyncWorker da loja. Ligado para o super admin de plataforma,
    /// que é ONLINE-only (usa as rotas /admin/*) e não sincroniza dados de
    /// loja — sem isso o worker levaria 401 nas rotas /sync/* e forçaria
    /// logout indevido.
    sync_paused: Arc<std::sync::atomic::AtomicBool>,
}

impl DesktopState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        company_id: Uuid,
        product_service: Arc<ProductService>,
        auth_service: Arc<AuthService>,
        company_service: Arc<CompanyService>,
        customer_service: Arc<CustomerService>,
        business_hours_service: Arc<BusinessHoursService>,
        category_service: Arc<CategoryService>,
        job_role_service: Arc<JobRoleService>,
        subcategory_service: Arc<SubcategoryService>,
        order_service: Arc<OrderService>,
        addon_group_service: Arc<AddonGroupService>,
        addon_service: Arc<AddonService>,
        banner_service: Arc<BannerService>,
        coupon_service: Arc<CouponService>,
        customer_address_service: Arc<CustomerAddressService>,
        printer_service: Arc<PrinterService>,
        cash_service: Arc<CashService>,
        finance_category_service: Arc<FinanceCategoryService>,
        finance_service: Arc<FinanceService>,
        wallet_service: Arc<WalletService>,
        subscription_service: Arc<SubscriptionService>,
        payment_method_service: Arc<PaymentMethodService>,
        reconcile: Arc<crate::repository::reconcile::SqliteReconcileRepository>,
        session: Arc<SessionStore>,
        sync_status: SyncStatusHandle,
        alarm_watcher: Arc<AlarmWatcher>,
        alarm_player: Arc<AlarmPlayer>,
        alarm_signal: Arc<Notify>,
    ) -> Self {
        Self {
            company_id: Arc::new(RwLock::new(company_id)),
            product_service,
            auth_service,
            company_service,
            customer_service,
            business_hours_service,
            category_service,
            job_role_service,
            subcategory_service,
            order_service,
            addon_group_service,
            addon_service,
            banner_service,
            coupon_service,
            customer_address_service,
            printer_service,
            cash_service,
            finance_category_service,
            finance_service,
            wallet_service,
            subscription_service,
            payment_method_service,
            reconcile,
            session,
            sync_status,
            alarm_watcher,
            alarm_player,
            alarm_signal,
            sync_paused: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Pausa/retoma o SyncWorker da loja (ligado p/ super admin).
    pub fn set_sync_paused(&self, paused: bool) {
        self.sync_paused
            .store(paused, std::sync::atomic::Ordering::Relaxed);
    }

    /// `true` se o SyncWorker deve pular o ciclo (super admin online-only).
    pub fn sync_paused(&self) -> bool {
        self.sync_paused.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Lê o company_id atual.
    pub fn company_id(&self) -> Uuid {
        *self.company_id.read().unwrap()
    }

    /// Atualiza o company_id (ex: após login no servidor).
    pub fn set_company_id(&self, id: Uuid) {
        *self.company_id.write().unwrap() = id;
    }
}
