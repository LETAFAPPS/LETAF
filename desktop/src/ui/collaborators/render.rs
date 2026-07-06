//! Construção dos modelos Slint da tela de Colaboradores + cache de edição.
//!
//! UI sem lógica (§1/§3): aqui só transformamos entidades do domínio em
//! structs de view. A autoridade (validação/permissão) vive no backend (§11).

use std::collections::HashSet;

use slint::{Color, ModelRc, SharedString, VecModel};
use uuid::Uuid;

use letaf_core::auth::model::User;
use letaf_core::job_role::model::JobRole;
use letaf_core::permission;

use crate::{CollabEmployeeRow, CollabPermRow, CollabRoleRow, MainWindow};

/// Estado de edição mantido no Rust (fonte de verdade do formulário de
/// Função, espelhando o padrão de cache das outras telas).
#[derive(Default)]
pub(crate) struct CollabCache {
    /// Permissões marcadas no formulário de Função em edição.
    pub(crate) editing_perms: HashSet<String>,
    /// Funções carregadas (para editar sem novo round-trip).
    pub(crate) roles: Vec<JobRole>,
    /// Funcionários carregados.
    pub(crate) employees: Vec<User>,
    /// Opções do combo de Função na ordem exibida (sem o "Sem função").
    pub(crate) role_options: Vec<(Uuid, String)>,
}

/// Aplica as listas (Funções + Funcionários) e o combo de Funções na UI,
/// atualizando o cache com os dados carregados.
pub(crate) fn apply_lists(ui: &MainWindow, cache: &CollabCache) {
    let role_rows: Vec<CollabRoleRow> = cache
        .roles
        .iter()
        .map(|r| {
            // Telas liberadas (com `.view`), na ordem do catálogo.
            let views: Vec<&'static str> = permission::FEATURES
                .iter()
                .filter(|(k, _, _)| r.permissions.iter().any(|p| p == &format!("{k}.view")))
                .map(|(_, label, _)| *label)
                .collect();
            let chips: Vec<SharedString> =
                views.iter().take(4).map(|s| SharedString::from(*s)).collect();
            CollabRoleRow {
                id: r.base.id.to_string().into(),
                name: r.name.clone().into(),
                perm_summary: format!("{} permissões ativas", views.len()).into(),
                perm_chips: ModelRc::new(VecModel::from(chips)),
            }
        })
        .collect();
    ui.set_collab_roles(ModelRc::new(VecModel::from(role_rows)));

    let mut combo: Vec<SharedString> = vec!["— Sem função —".into()];
    combo.extend(cache.role_options.iter().map(|(_, name)| name.clone().into()));
    ui.set_collab_role_combo(ModelRc::new(VecModel::from(combo)));

    let role_name = |id: Option<Uuid>| -> String {
        id.and_then(|jid| cache.roles.iter().find(|r| r.base.id == jid))
            .map(|r| r.name.clone())
            .unwrap_or_else(|| "".into())
    };
    let emp_rows: Vec<CollabEmployeeRow> = cache
        .employees
        .iter()
        .map(|u| CollabEmployeeRow {
            id: u.base.id.to_string().into(),
            name: u.name.clone().into(),
            email: u.email.clone().into(),
            role_label: u.role.label_pt_br().into(),
            job_role_name: role_name(u.job_role_id).into(),
            is_admin: u.role.is_admin(),
            initial: initial_of(&u.name).into(),
            avatar_color: avatar_color_for(&u.name),
        })
        .collect();
    ui.set_collab_employees(ModelRc::new(VecModel::from(emp_rows)));
}

/// Primeira letra do nome em maiúscula (avatar). "?" quando vazio.
fn initial_of(name: &str) -> String {
    name.trim()
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".into())
}

/// Cor determinística do avatar do funcionário (hash do nome → paleta).
fn avatar_color_for(name: &str) -> Color {
    let palette = [
        (0xE8, 0x73, 0x1C), // laranja
        (0x2E, 0x7D, 0x32), // verde
        (0x1E, 0x88, 0xE5), // azul
        (0x8E, 0x24, 0xAA), // roxo
        (0xC2, 0x18, 0x5B), // rosa
        (0x00, 0x89, 0x7B), // teal
    ];
    let mut h: u32 = 0;
    for b in name.bytes() {
        h = h.wrapping_mul(31).wrapping_add(b as u32);
    }
    let (r, g, b) = palette[(h as usize) % palette.len()];
    Color::from_rgb_u8(r, g, b)
}

/// Descrição curta de cada feature (pt-BR) exibida nos cards de permissão.
fn perm_sub(key: &str) -> &'static str {
    match key {
        "dashboard" => "Dashboard",
        "reports" => "Analytics",
        "pdv" => "Ponto de venda",
        "orders" => "Gestão",
        "cash" => "Controle",
        "finance" => "Fluxo",
        "products" => "Cardápio",
        "stock" => "Controle",
        "addons" => "Itens extras",
        "categories" => "Grupos",
        "banners" => "Imagens",
        "coupons" => "Desconto",
        "customers" => "Cadastro",
        "collaborators" => "Equipe",
        "subscription" => "Plano",
        _ => "",
    }
}

/// Reconstrói a matriz de permissões a partir do conjunto em edição.
pub(crate) fn apply_perm_rows(ui: &MainWindow, perms: &HashSet<String>) {
    let rows: Vec<CollabPermRow> = permission::FEATURES
        .iter()
        .map(|(key, label, has_edit)| CollabPermRow {
            key: (*key).into(),
            label: (*label).into(),
            sub_label: perm_sub(key).into(),
            has_edit: *has_edit,
            view_on: perms.contains(&format!("{key}.view")),
            edit_on: perms.contains(&format!("{key}.edit")),
        })
        .collect();
    ui.set_collab_perm_rows(ModelRc::new(VecModel::from(rows)));

    // Contagem "X de Y" exibida no cabeçalho do editor (telas com `.view`).
    let active = permission::FEATURES
        .iter()
        .filter(|(k, _, _)| perms.contains(&format!("{k}.view")))
        .count();
    ui.set_collab_perm_active(active as i32);
    ui.set_collab_perm_total(permission::FEATURES.len() as i32);
}
