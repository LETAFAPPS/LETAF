//! Impersonation: o super admin "entra" numa empresa (sessão com escopo do
//! tenant, como o proprietário) e usa as telas reais do ERP; "Sair" restaura
//! a sessão do super admin. A autoridade é o backend (§11) — o token com
//! escopo é o que concede o acesso.

use std::sync::{Arc, Mutex};

use letaf_core::auth::model::UserRole;
use slint::{ComponentHandle, SharedString};
use tokio::sync::{Notify, RwLock};
use uuid::Uuid;

use crate::context::DesktopState;
use crate::ui::auth::{apply_login, update_ui_after_login};
use crate::{AdminState, MainWindow, HTTP_CLIENT};

use super::super::helpers::show_toast;
use super::dto::ImpersonateDto;

/// Snapshot da sessão do super admin, para restaurar ao sair da empresa.
#[derive(Clone)]
struct SaSession {
    token: String,
    company_id: Uuid,
    subdomain: String,
    name: String,
    perms: Vec<String>,
}

/// Dependências compartilhadas pelos dois lados (entrar/sair).
#[derive(Clone)]
struct ImpersonationCtx {
    state: DesktopState,
    auth_token: Arc<RwLock<Option<String>>>,
    notify: Arc<Notify>,
    server_url: String,
    /// Snapshot compartilhado entre "entrar" e "sair".
    snapshot: Arc<Mutex<Option<SaSession>>>,
}

/// Registra "entrar na empresa" e "sair" (restaura o super admin).
pub(super) fn setup_impersonation(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    sync_notify: Arc<Notify>,
    auth_token: &Arc<RwLock<Option<String>>>,
    server_url: &str,
) {
    let ctx = ImpersonationCtx {
        state: state.clone(),
        auth_token: auth_token.clone(),
        notify: sync_notify,
        server_url: server_url.to_string(),
        snapshot: Arc::new(Mutex::new(None)),
    };
    setup_enter(ui, handle, ctx.clone());
    setup_exit(ui, handle, ctx);
}

/// ── Entrar na empresa ──
fn setup_enter(ui: &MainWindow, handle: &tokio::runtime::Handle, ctx: ImpersonationCtx) {
    let ui_weak = ui.as_weak();
    let handle = handle.clone();
    ui.global::<AdminState>().on_company_impersonate(move |id| {
        let id = id.to_string();
        let ui_weak = ui_weak.clone();
        let ctx = ctx.clone();
        handle.spawn(async move { enter_company(ctx, id, ui_weak).await });
    });
}

async fn enter_company(ctx: ImpersonationCtx, id: String, ui_weak: slint::Weak<MainWindow>) {
    // Snapshot da sessão atual do super admin (para o "Sair").
    let Some(sa) = capture_sa_session(&ctx).await else {
        // Só super admin gerencia (o backend também exige — §11).
        return;
    };
    let Some(dto) = fetch_impersonation(&ctx, &sa.token, &id).await else {
        let uw = ui_weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = uw.upgrade() {
                show_toast(&ui, "Não foi possível entrar na empresa", "error");
            }
        });
        return;
    };
    *ctx.snapshot.lock().unwrap() = Some(sa);
    apply_company_session(&ctx, dto, ui_weak).await;
}

/// Sessão corrente do super admin; `None` se quem chamou não é super admin.
async fn capture_sa_session(ctx: &ImpersonationCtx) -> Option<SaSession> {
    let (is_admin, is_super, sa_perms) = ctx.state.session.load_perms().await;
    let _ = is_admin;
    let sa = SaSession {
        token: ctx.auth_token.read().await.clone().unwrap_or_default(),
        company_id: ctx.state.company_id(),
        subdomain: ctx.state.session.load_subdomain().await.unwrap_or_default(),
        name: ctx.state.session.load_user_name().await.unwrap_or_default(),
        perms: sa_perms,
    };
    is_super.then_some(sa)
}

/// POST /admin/companies/{id}/impersonate → token com escopo do tenant.
async fn fetch_impersonation(
    ctx: &ImpersonationCtx,
    sa_token: &str,
    id: &str,
) -> Option<ImpersonateDto> {
    let server_url = &ctx.server_url;
    let resp = HTTP_CLIENT
        .post(format!("{server_url}/admin/companies/{id}/impersonate"))
        .bearer_auth(sa_token)
        .send()
        .await;
    match resp {
        Ok(r) if r.status().is_success() => r.json().await.ok(),
        _ => None,
    }
}

/// Troca a sessão local para a empresa e atualiza a UI.
async fn apply_company_session(
    ctx: &ImpersonationCtx,
    dto: ImpersonateDto,
    ui_weak: slint::Weak<MainWindow>,
) {
    // A troca de tenant NÃO é atômica: `apply_login` tem awaits
    // entre gravar o token e trocar o `company_id`. Com o worker
    // rodando, um tick nessa janela usaria o company_id de um
    // tenant com o JWT de outro. Despausa só DEPOIS — é o que o
    // caminho de SAÍDA da impersonation já fazia.
    apply_login(
        &ctx.state, &ctx.auth_token, &ctx.notify,
        dto.user.company_id, &dto.company_name, &dto.subdomain, dto.token,
    )
    .await;
    // Empresa comum é offline-first: retoma o sync para popular os
    // dados do tenant no SQLite local.
    ctx.state.set_sync_paused(false);
    ctx.state.session.save_perms(true, false, &dto.perms).await;
    ctx.state.session.save_user_name(&dto.user.name).await;
    let company_name = dto.company_name;
    update_ui_after_login(ui_weak.clone(), UserRole::Admin, dto.perms, dto.user.name);
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_impersonating(true);
            ui.set_impersonating_company(company_name.into());
        }
    });
}

/// ── Sair (restaura o super admin) ──
fn setup_exit(ui: &MainWindow, handle: &tokio::runtime::Handle, ctx: ImpersonationCtx) {
    let ui_weak = ui.as_weak();
    let handle = handle.clone();
    ui.on_exit_impersonation(move || {
        let Some(sa) = ctx.snapshot.lock().unwrap().take() else { return };
        let ui_weak = ui_weak.clone();
        let ctx = ctx.clone();
        handle.spawn(async move { exit_company(ctx, sa, ui_weak).await });
    });
}

async fn exit_company(ctx: ImpersonationCtx, sa: SaSession, ui_weak: slint::Weak<MainWindow>) {
    // Super admin é online-only: pausa o sync antes do apply_login.
    ctx.state.set_sync_paused(true);
    apply_login(
        &ctx.state, &ctx.auth_token, &ctx.notify,
        sa.company_id, "", &sa.subdomain, sa.token,
    )
    .await;
    ctx.state.session.save_perms(true, true, &sa.perms).await;
    ctx.state.session.save_user_name(&sa.name).await;
    update_ui_after_login(ui_weak.clone(), UserRole::SuperAdmin, sa.perms, sa.name);
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_impersonating(false);
            ui.set_impersonating_company(SharedString::new());
        }
    });
}
