use std::collections::HashMap;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use uuid::Uuid;


use crate::context::DesktopState;
use crate::{MainWindow, SubcategoryData};

use super::crud::to_subcategory_data;

/// Carrega subcategorias do SQLite e popula a tabela.
///
/// Para resolver o nome legível da categoria (exibido na coluna
/// "Categoria"), buscamos as categorias e fazemos o join em memória —
/// evita um SELECT por linha e mantém o repository simples (§10).
pub(crate) fn setup_refresh_subcategories(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_refresh_subcategories(move || {
        let ui_weak = ui_weak.clone();
        let state = state.clone();

        handle.spawn(async move {
            let cid = state.company_id();
            let cats_result = state.category_service.find_all(cid).await;
            let subs_result = state.subcategory_service.find_all(cid).await;

            match (cats_result, subs_result) {
                (Ok(cats), Ok(subs)) => {
                    let cat_names: HashMap<Uuid, String> = cats
                        .iter()
                        .map(|c| (c.base.id, c.name.clone()))
                        .collect();
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        let filter = ui.get_subcategory_filter_category_id().to_string();
                        let filter_id = Uuid::parse_str(&filter).ok();
                        let data: Vec<SubcategoryData> = subs
                            .iter()
                            .filter(|s| filter_id.is_none_or(|fid| s.category_id == fid))
                            .map(|s| to_subcategory_data(s, &cat_names))
                            .collect();
                        ui.set_subcategories(ModelRc::new(VecModel::from(data)));
                    });
                }
                (Err(e), _) | (_, Err(e)) => {
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = ui_weak.upgrade() else { return };
                        ui.set_status_message(SharedString::from(format!("Erro: {e}")));
                    });
                }
            }
        });
    });
}

/// Limpa erros de validação do formulário de subcategoria.
pub(crate) fn clear_subcategory_errors(ui: &MainWindow) {
    ui.set_subcategory_error_name(SharedString::default());
    ui.set_subcategory_error_category(SharedString::default());
}

/// Limpa o formulário de subcategoria.
pub(crate) fn clear_subcategory_form(ui: &MainWindow) {
    ui.set_subcategory_name(SharedString::default());
    ui.set_subcategory_category_id(SharedString::default());
    ui.set_subcategory_category_name(SharedString::default());
    ui.set_subcategory_category_open(false);
    clear_subcategory_errors(ui);
}

/// Valida campos obrigatórios do formulário de subcategoria.
///
/// Regras aplicadas (AI_RULES.md §11): nome obrigatório, categoria
/// obrigatória. A validação cross-tenant (categoria pertence à empresa)
/// fica no service.
pub(crate) fn validate_subcategory_form(ui: &MainWindow) -> bool {
    let mut valid = true;
    clear_subcategory_errors(ui);

    if ui.get_subcategory_name().trim().is_empty() {
        ui.set_subcategory_error_name(SharedString::from("Preencha o nome da subcategoria"));
        valid = false;
    }
    if ui.get_subcategory_category_id().trim().is_empty() {
        ui.set_subcategory_error_category(SharedString::from("Selecione uma categoria"));
        valid = false;
    }

    valid
}

