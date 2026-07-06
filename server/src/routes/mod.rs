pub mod addon_groups;
pub mod admin;
pub mod app_update;
pub mod auth;
pub mod banners;
pub mod catalog;
pub mod categories;
pub mod collaborators;
pub mod coupons;
pub mod customer_address;
pub mod customer_auth;
pub mod customers;
pub mod health;
pub mod job_roles;
pub mod orders;
pub mod payment_methods;
pub mod payments;
pub mod products;
pub mod subcategories;
pub mod subscriptions;
pub mod sync;

use axum::Router;

use crate::context::AppState;

pub fn create_routes() -> Router<AppState> {
    Router::new()
        .merge(auth::routes())
        .merge(admin::routes())
        .merge(app_update::routes())
        .merge(health::routes())
        .merge(products::routes())
        .merge(customers::routes())
        .merge(categories::routes())
        .merge(subcategories::routes())
        .merge(job_roles::routes())
        .merge(collaborators::routes())
        .merge(catalog::routes())
        .merge(customer_auth::routes())
        .merge(customer_address::routes())
        .merge(orders::routes())
        .merge(addon_groups::routes())
        .merge(banners::routes())
        .merge(coupons::routes())
        .merge(payment_methods::routes())
        .merge(payments::routes())
        .merge(subscriptions::routes())
        .merge(sync::routes())
}
