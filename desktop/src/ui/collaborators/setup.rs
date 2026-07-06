//! Callbacks da tela de Colaboradores (RBAC) — Funções + Funcionários.
//!
//! Regras (AI_RULES.md §1, §3, §7, §11):
//! - UI não tem lógica: callbacks delegam aos services (offline-first).
//! - Acesso ao banco só via service/repository; isolamento por company_id.
//! - O backend é a autoridade; esta tela é conveniência de cadastro.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use slint::{ComponentHandle, SharedString};
use tokio::sync::Notify;
use uuid::Uuid;

use crate::context::DesktopState;
use crate::MainWindow;

use super::super::helpers::show_toast;
use super::render::{apply_lists, apply_perm_rows, CollabCache};

/// Registra todos os callbacks da tela de Colaboradores.
pub(crate) fn setup_collaborators(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
) {
    let cache: Arc<Mutex<CollabCache>> = Arc::new(Mutex::new(CollabCache::default()));

    setup_refresh(ui, state, handle, &cache);
    setup_role_form(ui, &cache);
    setup_role_persist(ui, state, handle, &cache, &sync_notify);
    setup_employee_form(ui, &cache);
    setup_employee_persist(ui, state, handle, &cache, &sync_notify);
}

/// Recarrega Funções + Funcionários do SQLite e reaplica os modelos.
fn setup_refresh(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    cache: &Arc<Mutex<CollabCache>>,
) {
    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();
    let cache = cache.clone();
    ui.on_refresh_collaborators(move || {
        let ui_weak = ui_weak.clone();
        let state = state.clone();
        let cache = cache.clone();
        handle.spawn(async move {
            let cid = state.company_id();
            let roles = state.job_role_service.find_all(cid).await;
            let employees = state.auth_service.find_all(cid).await;
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui_weak.upgrade() else { return };
                match (roles, employees) {
                    (Ok(roles), Ok(employees)) => {
                        if let Ok(mut g) = cache.lock() {
                            g.role_options = roles.iter().map(|r| (r.base.id, r.name.clone())).collect();
                            g.roles = roles;
                            g.employees = employees;
                            apply_lists(&ui, &g);
                            let perms = g.editing_perms.clone();
                            apply_perm_rows(&ui, &perms);
                        }
                    }
                    (Err(e), _) | (_, Err(e)) => {
                        ui.set_status_message(SharedString::from(format!("Erro ao carregar colaboradores: {e}")));
                    }
                }
            });
        });
    });
}

/// Callbacks síncronos do formulário de Função (novo/editar/marcar perm).
fn setup_role_form(ui: &MainWindow, cache: &Arc<Mutex<CollabCache>>) {
    // Nova função: limpa o formulário.
    {
        let ui_weak = ui.as_weak();
        let cache = cache.clone();
        ui.on_collab_new_role(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            if let Ok(mut g) = cache.lock() {
                g.editing_perms.clear();
                ui.set_collab_editing_role_id(SharedString::from(""));
                ui.set_collab_role_name(SharedString::from(""));
                apply_perm_rows(&ui, &g.editing_perms);
            }
        });
    }
    // Editar função: carrega nome + permissões no formulário.
    {
        let ui_weak = ui.as_weak();
        let cache = cache.clone();
        ui.on_collab_edit_role(move |id_str| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let Ok(id) = Uuid::parse_str(id_str.as_str()) else { return };
            if let Ok(mut g) = cache.lock() {
                if let Some(role) = g.roles.iter().find(|r| r.base.id == id) {
                    let perms: HashSet<String> = role.permissions.iter().cloned().collect();
                    let name = role.name.clone();
                    g.editing_perms = perms;
                    ui.set_collab_editing_role_id(id_str);
                    ui.set_collab_role_name(SharedString::from(name));
                    apply_perm_rows(&ui, &g.editing_perms);
                }
            }
        });
    }
    // Marcar tudo / limpar tudo: se todas as telas já têm `.view`,
    // limpa; senão libera todas (view + edit das que têm edição).
    {
        let ui_weak = ui.as_weak();
        let cache = cache.clone();
        ui.on_collab_mark_all_perms(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            if let Ok(mut g) = cache.lock() {
                let all_on = letaf_core::permission::FEATURES
                    .iter()
                    .all(|(k, _, _)| g.editing_perms.contains(&format!("{k}.view")));
                if all_on {
                    g.editing_perms.clear();
                } else {
                    for (k, _, has_edit) in letaf_core::permission::FEATURES {
                        g.editing_perms.insert(format!("{k}.view"));
                        if *has_edit {
                            g.editing_perms.insert(format!("{k}.edit"));
                        }
                    }
                }
                apply_perm_rows(&ui, &g.editing_perms);
            }
        });
    }
    // Marcar/desmarcar uma permissão (view/edit) da feature.
    {
        let ui_weak = ui.as_weak();
        let cache = cache.clone();
        ui.on_collab_toggle_perm(move |key, is_edit, on| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let view = format!("{key}.view");
            let edit = format!("{key}.edit");
            if let Ok(mut g) = cache.lock() {
                match (is_edit, on) {
                    // Editar marcado ⇒ Ver também (não dá pra editar sem ver).
                    (true, true) => {
                        g.editing_perms.insert(edit);
                        g.editing_perms.insert(view);
                    }
                    (true, false) => {
                        g.editing_perms.remove(&edit);
                    }
                    (false, true) => {
                        g.editing_perms.insert(view);
                    }
                    // Ver desmarcado ⇒ remove Editar junto (consistência §11).
                    (false, false) => {
                        g.editing_perms.remove(&view);
                        g.editing_perms.remove(&edit);
                    }
                }
                apply_perm_rows(&ui, &g.editing_perms);
            }
        });
    }
}

/// Salvar/excluir Função (assíncrono → recarrega no fim).
fn setup_role_persist(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    cache: &Arc<Mutex<CollabCache>>,
    sync_notify: &Arc<Notify>,
) {
    // Salvar (cria ou atualiza).
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        let handle = handle.clone();
        let cache = cache.clone();
        let sync_notify = sync_notify.clone();
        ui.on_collab_save_role(move |id_str, name| {
            let name = name.trim().to_string();
            if name.is_empty() {
                if let Some(ui) = ui_weak.upgrade() {
                    show_toast(&ui, "Informe o nome da função", "error");
                    ui.set_status_message(SharedString::from("Informe o nome da função"));
                }
                return;
            }
            let perms: Vec<String> = cache
                .lock()
                .map(|g| g.editing_perms.iter().cloned().collect())
                .unwrap_or_default();
            let ui_weak = ui_weak.clone();
            let state = state.clone();
            let notify = sync_notify.clone();
            handle.spawn(async move {
                let cid = state.company_id();
                let res = if id_str.is_empty() {
                    state.job_role_service.create(cid, name, perms).await.map(|_| ())
                } else if let Ok(id) = Uuid::parse_str(id_str.as_str()) {
                    state.job_role_service.update(cid, id, name, perms).await.map(|_| ())
                } else {
                    Ok(())
                };
                // Sync imediata (§7.4): a função alterada sobe agora, para que
                // qualquer funcionário que logue em seguida já receba as novas
                // permissões (o servidor as resolve ao vivo pela função) —
                // sem depender do ciclo periódico nem de reeditar o funcionário.
                if res.is_ok() { notify.notify_one(); }
                report_and_refresh(ui_weak, res, "Função salva");
            });
        });
    }
    // Excluir Função é tratado pelo modal de confirmação global
    // (`request-delete` → `confirm-delete` em ui/mod.rs).
}

/// Callbacks síncronos do formulário de Funcionário.
fn setup_employee_form(ui: &MainWindow, cache: &Arc<Mutex<CollabCache>>) {
    // Novo funcionário: limpa o formulário.
    {
        let ui_weak = ui.as_weak();
        ui.on_collab_new_employee(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            ui.set_collab_emp_id(SharedString::from(""));
            ui.set_collab_emp_name(SharedString::from(""));
            ui.set_collab_emp_email(SharedString::from(""));
            ui.set_collab_emp_password(SharedString::from(""));
            ui.set_collab_emp_role_index(0);
        });
    }
    // Editar funcionário: preenche o formulário (senha em branco).
    {
        let ui_weak = ui.as_weak();
        let cache = cache.clone();
        ui.on_collab_edit_employee(move |id_str| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let Ok(id) = Uuid::parse_str(id_str.as_str()) else { return };
            if let Ok(g) = cache.lock() {
                if let Some(u) = g.employees.iter().find(|u| u.base.id == id) {
                    // Índice no combo: 0 = sem função; senão posição+1.
                    let idx = u
                        .job_role_id
                        .and_then(|jid| g.role_options.iter().position(|(rid, _)| *rid == jid))
                        .map(|p| (p + 1) as i32)
                        .unwrap_or(0);
                    ui.set_collab_emp_id(id_str);
                    ui.set_collab_emp_name(SharedString::from(u.name.clone()));
                    ui.set_collab_emp_email(SharedString::from(u.email.clone()));
                    ui.set_collab_emp_password(SharedString::from(""));
                    ui.set_collab_emp_role_index(idx);
                }
            }
        });
    }
}

/// Salvar/excluir Funcionário (assíncrono → recarrega no fim).
fn setup_employee_persist(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    cache: &Arc<Mutex<CollabCache>>,
    sync_notify: &Arc<Notify>,
) {
    // Salvar (cria ou atualiza).
    {
        let ui_weak = ui.as_weak();
        let state = state.clone();
        let handle = handle.clone();
        let cache = cache.clone();
        let sync_notify = sync_notify.clone();
        ui.on_collab_save_employee(move |id_str, name, email, password, role_idx| {
            let name = name.trim().to_string();
            let email = email.trim().to_string();
            if name.is_empty() || (id_str.is_empty() && email.is_empty()) {
                if let Some(ui) = ui_weak.upgrade() {
                    show_toast(&ui, "Informe nome e e-mail do funcionário", "error");
                    ui.set_status_message(SharedString::from("Informe nome e e-mail do funcionário"));
                }
                return;
            }
            // Mapeia o índice do combo → Função (0 = sem função).
            let job_role_id: Option<Uuid> = if role_idx <= 0 {
                None
            } else {
                cache
                    .lock()
                    .ok()
                    .and_then(|g| g.role_options.get((role_idx - 1) as usize).map(|(id, _)| *id))
            };
            let password = password.to_string();
            let ui_weak = ui_weak.clone();
            let state = state.clone();
            let notify = sync_notify.clone();
            handle.spawn(async move {
                let cid = state.company_id();
                let res = if id_str.is_empty() {
                    state
                        .auth_service
                        .create_employee(cid, email, password, name, job_role_id)
                        .await
                        .map(|_| ())
                } else if let Ok(id) = Uuid::parse_str(id_str.as_str()) {
                    let pwd = if password.trim().is_empty() { None } else { Some(password) };
                    state
                        .auth_service
                        .update_employee(cid, id, name, job_role_id, pwd)
                        .await
                        .map(|_| ())
                } else {
                    Ok(())
                };
                // Sync imediata (§7.4): o funcionário criado/alterado sobe agora.
                if res.is_ok() { notify.notify_one(); }
                report_and_refresh(ui_weak, res, "Funcionário salvo");
            });
        });
    }
    // Excluir Funcionário é tratado pelo modal de confirmação global
    // (`request-delete` → `confirm-delete` em ui/mod.rs).
}

/// Mostra feedback e recarrega a tela após uma operação de persistência.
fn report_and_refresh(
    ui_weak: slint::Weak<MainWindow>,
    res: Result<(), letaf_core::error::CoreError>,
    ok_msg: &'static str,
) {
    let _ = slint::invoke_from_event_loop(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        match res {
            Ok(()) => {
                show_toast(&ui, ok_msg, "success");
                ui.set_status_message(SharedString::from(ok_msg));
                ui.invoke_refresh_collaborators();
            }
            Err(e) => {
                let msg = format!("Erro: {e}");
                show_toast(&ui, &msg, "error");
                ui.set_status_message(SharedString::from(msg));
            }
        }
    });
}
