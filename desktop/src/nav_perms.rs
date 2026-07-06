//! Mapeia as permissões efetivas do operador (RBAC §11) para o struct
//! `NavPerms` que a UI Slint usa para esconder/mostrar itens da sidebar.
//!
//! Esconder itens é apenas UX — o servidor revalida cada rota (§11). As
//! chaves seguem o catálogo de `letaf_core::permission` ("feature.view").
//! Atenção ao mapeamento: a feature "stock" do catálogo corresponde à aba
//! "inventory" (Estoque) da UI.

use crate::NavPerms;

/// Constrói `NavPerms` a partir do papel e das permissões do operador.
/// Admin/SuperAdmin (`is_admin`) liberam todas as abas.
pub fn nav_perms_from(is_admin: bool, is_super_admin: bool, perms: &[String]) -> NavPerms {
    // O super admin de plataforma NÃO acessa telas da loja — só os menus
    // próprios do painel. Portanto TODO flag de loja é `false` para ele,
    // ignorando inclusive as `perms` (o servidor devolve todas as perms
    // para admin/super admin — sem este curto-circuito os menus da loja
    // reapareceriam). Telas da loja liberam por admin de loja ou por
    // permissão explícita do funcionário.
    let has = |key: &str| {
        if is_super_admin {
            return false;
        }
        is_admin || perms.iter().any(|p| p == key)
    };
    NavPerms {
        is_admin,
        is_super_admin,
        dashboard: has("dashboard.view"),
        reports: has("reports.view"),
        pdv: has("pdv.view"),
        orders: has("orders.view"),
        cash: has("cash.view"),
        finance: has("finance.view"),
        products: has("products.view"),
        inventory: has("stock.view"),
        addons: has("addons.view"),
        categories: has("categories.view"),
        banners: has("banners.view"),
        coupons: has("coupons.view"),
        customers: has("customers.view"),
        collaborators: has("collaborators.view"),
        subscription: has("subscription.view"),
    }
}

/// Primeira aba que o operador pode abrir, na ordem do menu lateral.
/// Admin começa no Dashboard; um funcionário começa na primeira aba
/// liberada (evita abrir uma tela sem permissão após o login).
pub fn first_accessible_tab(is_admin: bool, is_super_admin: bool, perms: &[String]) -> &'static str {
    if is_super_admin {
        return "admin-overview";
    }
    if is_admin {
        return "dashboard";
    }
    // (permissão de view, aba) — mesma ordem da sidebar.
    const ORDER: &[(&str, &str)] = &[
        ("dashboard.view", "dashboard"),
        ("reports.view", "reports"),
        ("pdv.view", "pdv"),
        ("orders.view", "orders"),
        ("cash.view", "cash"),
        ("finance.view", "finance"),
        ("products.view", "products"),
        ("stock.view", "inventory"),
        ("addons.view", "addons"),
        ("categories.view", "categories"),
        ("banners.view", "banners"),
        ("coupons.view", "coupons"),
        ("customers.view", "customers"),
        ("collaborators.view", "collaborators"),
        ("subscription.view", "subscription"),
    ];
    ORDER
        .iter()
        .find(|(key, _)| perms.iter().any(|p| p == key))
        .map(|(_, tab)| *tab)
        // Sem nenhuma permissão de visualização: fallback seguro.
        .unwrap_or("pdv")
}
