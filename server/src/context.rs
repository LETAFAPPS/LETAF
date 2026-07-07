use std::sync::Arc;

use sqlx::PgPool;

use letaf_core::addon::service::AddonService;
use letaf_core::addon_group::service::AddonGroupService;
use letaf_core::auth::service::AuthService;
use letaf_core::password_reset::service::PasswordResetService;
use letaf_core::plan::service::PlanService;
use letaf_core::banner::service::BannerService;
use letaf_core::business_hours::service::BusinessHoursService;
use letaf_core::cash::service::CashService;
use letaf_core::coupon::service::CouponService;
use letaf_core::category::service::CategoryService;
use letaf_core::job_role::service::JobRoleService;
use letaf_core::subcategory::service::SubcategoryService;
use letaf_core::subscription::service::SubscriptionService;
use letaf_core::subscription::card_billing::CardBillingService;
use letaf_core::subscription::pix_auto_billing::PixAutoBillingService;
use letaf_core::payment_gateway::service::PaymentService;
use letaf_core::payment_method::service::PaymentMethodService;
use letaf_core::company::service::CompanyService;
use letaf_core::customer::service::CustomerService;
use letaf_core::product::service::ProductService;
use letaf_core::customer_address::service::CustomerAddressService;
use letaf_core::finance::service::FinanceService;
use letaf_core::finance_category::service::FinanceCategoryService;
use letaf_core::order::service::OrderService;
use letaf_core::wallet::service::WalletService;

use crate::config::AppConfig;

/// Estado compartilhado da aplicação (AppState).
///
/// Regras aplicadas (AI_RULES.md §1, §4, §9):
/// - Backend usa axum + Tokio + SQLx
/// - É PROIBIDO misturar responsabilidades entre camadas
/// - Services encapsulam repositories (inversão de dependência)
///
/// Contém pool, config e services de domínio.
/// Injetado como State no axum Router.
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub config: AppConfig,
    pub product_service: Arc<ProductService>,
    pub auth_service: Arc<AuthService>,
    pub company_service: Arc<CompanyService>,
    pub customer_service: Arc<CustomerService>,
    pub business_hours_service: Arc<BusinessHoursService>,
    pub category_service: Arc<CategoryService>,
    pub job_role_service: Arc<JobRoleService>,
    pub subcategory_service: Arc<SubcategoryService>,
    pub order_service: Arc<OrderService>,
    pub customer_address_service: Arc<CustomerAddressService>,
    pub addon_group_service: Arc<AddonGroupService>,
    pub addon_service: Arc<AddonService>,
    pub banner_service: Arc<BannerService>,
    pub coupon_service: Arc<CouponService>,
    pub cash_service: Arc<CashService>,
    pub finance_category_service: Arc<FinanceCategoryService>,
    pub finance_service: Arc<FinanceService>,
    pub wallet_service: Arc<WalletService>,
    pub subscription_service: Arc<SubscriptionService>,
    pub payment_method_service: Arc<PaymentMethodService>,
    /// `None` quando o gateway Efi não estiver configurado (EFI_* vazias).
    /// Endpoints `/payments/*` retornam 503 nesse caso.
    pub payment_service: Option<Arc<PaymentService>>,
    /// `None` quando a API Cobranças (cartão) não estiver configurada.
    /// Endpoints `/subscription/card*` e `/webhooks/efi` retornam 503.
    pub card_billing: Option<Arc<CardBillingService>>,
    /// Pix Automático (reusa a API PIX). `None` quando EFI_* (PIX) não
    /// estiver configurada. Endpoints `/subscription/pix-auto*` → 503.
    pub pix_auto: Option<Arc<PixAutoBillingService>>,
    /// Sessões efêmeras do cadastro de cartão via página hosted (Efi.js).
    pub card_sessions: Arc<crate::card_session::CardSessionStore>,
    /// Recuperação de senha (código por e-mail) — fluxo "esqueci a senha".
    pub password_reset_service: Arc<PasswordResetService>,
    /// Catálogo de planos (gerido pelo super admin; lido pelas lojas).
    pub plan_service: Arc<PlanService>,
    /// Rate limiter dos endpoints de autenticação (anti-brute-force §11).
    pub login_rate_limiter: Arc<crate::rate_limit::RateLimiter>,
}

/// Máx. de tentativas de auth por IP dentro da janela. Generoso para não
/// travar escritório atrás de NAT, mas restritivo para bot (o bcrypt cost 13
/// já encarece cada tentativa).
const LOGIN_RATE_MAX: usize = 20;
const LOGIN_RATE_WINDOW_SECS: u64 = 60;

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pool: PgPool,
        config: AppConfig,
        product_service: Arc<ProductService>,
        auth_service: Arc<AuthService>,
        company_service: Arc<CompanyService>,
        customer_service: Arc<CustomerService>,
        business_hours_service: Arc<BusinessHoursService>,
        category_service: Arc<CategoryService>,
        job_role_service: Arc<JobRoleService>,
        subcategory_service: Arc<SubcategoryService>,
        order_service: Arc<OrderService>,
        customer_address_service: Arc<CustomerAddressService>,
        addon_group_service: Arc<AddonGroupService>,
        addon_service: Arc<AddonService>,
        banner_service: Arc<BannerService>,
        coupon_service: Arc<CouponService>,
        cash_service: Arc<CashService>,
        finance_category_service: Arc<FinanceCategoryService>,
        finance_service: Arc<FinanceService>,
        wallet_service: Arc<WalletService>,
        subscription_service: Arc<SubscriptionService>,
        payment_method_service: Arc<PaymentMethodService>,
        payment_service: Option<Arc<PaymentService>>,
        card_billing: Option<Arc<CardBillingService>>,
        pix_auto: Option<Arc<PixAutoBillingService>>,
        card_sessions: Arc<crate::card_session::CardSessionStore>,
        password_reset_service: Arc<PasswordResetService>,
        plan_service: Arc<PlanService>,
    ) -> Self {
        Self {
            login_rate_limiter: Arc::new(crate::rate_limit::RateLimiter::new(
                LOGIN_RATE_MAX,
                std::time::Duration::from_secs(LOGIN_RATE_WINDOW_SECS),
            )),
            pool,
            config,
            product_service,
            auth_service,
            company_service,
            customer_service,
            business_hours_service,
            category_service,
            job_role_service,
            subcategory_service,
            order_service,
            customer_address_service,
            addon_group_service,
            addon_service,
            banner_service,
            coupon_service,
            cash_service,
            finance_category_service,
            finance_service,
            wallet_service,
            subscription_service,
            payment_method_service,
            payment_service,
            card_billing,
            pix_auto,
            card_sessions,
            password_reset_service,
            plan_service,
        }
    }
}
