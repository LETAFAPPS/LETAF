use std::sync::Arc;

use reqwest::StatusCode;
use slint::{ComponentHandle, SharedString};
use tokio::sync::{Notify, RwLock};
use uuid::Uuid;

use letaf_core::auth::model::UserRole;

use crate::HTTP_CLIENT;
use crate::MainWindow;
use crate::context::DesktopState;

/// Callback: autentica no servidor e grava JWT.
///
/// Regras aplicadas (AI_RULES.md §4, §7.4, §8, §11):
/// - Desktop envia apenas email + password ao servidor
/// - Servidor identifica empresa automaticamente pelo email
/// - Retorna JWT, company_id e subdomain
/// - Lógica pós-login delegada a funções separadas (§8 — max 30-50 linhas)
pub(crate) fn setup_login(
    ui: &MainWindow,
    state: &DesktopState,
    handle: &tokio::runtime::Handle,
    auth_token: Arc<RwLock<Option<String>>>,
    sync_notify: Arc<Notify>,
    server_url: String,
) {
    // Login por PIN do operador — funcionalidade futura. Por ora só
    // informa que estará disponível (sem backend ainda).
    {
        let ui_weak = ui.as_weak();
        ui.on_login_with_pin(move || {
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_login_status(SharedString::from(
                    "Entrar com PIN do operador estará disponível em breve.",
                ));
            }
        });
    }

    let ui_weak = ui.as_weak();
    let state = state.clone();
    let handle = handle.clone();

    ui.on_do_login(move || {
        let Some(ui_ref) = ui_weak.upgrade() else { return };
        let email = ui_ref.get_login_email().to_string();
        let password = ui_ref.get_login_password().to_string();
        let remember_me = ui_ref.get_login_remember_me();

        if let Some(msg) = validate_login_fields(&email, &password) {
            ui_ref.set_login_status(SharedString::from(msg));
            return;
        }

        ui_ref.set_login_status(SharedString::from("Verificando..."));

        let ui_weak = ui_ref.as_weak();
        let state = state.clone();
        let auth_token = auth_token.clone();
        let notify = sync_notify.clone();
        let url = server_url.clone();

        handle.spawn(async move {
            let result = do_login(&url, &email, &password).await;

            match result {
                Ok(login) => {
                    // Super admin é online-only: pausa o sync da loja ANTES do
                    // apply_login (que dispara sync via notify). Evita 401 →
                    // logout indevido. Um login de loja depois retoma o sync.
                    state.set_sync_paused(login.role.is_super_admin());
                    if remember_me {
                        // Guarda só o email — a senha não é persistida (§11).
                        state.session.save_remember_me(&email).await;
                    } else {
                        state.session.clear_remember_me().await;
                    }
                    apply_login(&state, &auth_token, &notify, login.company_id, &login.company_name, &login.subdomain, login.token).await;
                    // Persiste as permissões para a gating sobreviver a um
                    // restart offline (§7/§11).
                    let is_admin = login.role.is_admin();
                    state.session.save_perms(is_admin, login.role.is_super_admin(), &login.perms).await;
                    state.session.save_user_name(&login.name).await;
                    // Limpa a foto em cache do operador anterior (a resposta de
                    // login não traz avatar; será buscada no /auth/me).
                    state.session.save_user_avatar("").await;
                    update_ui_after_login(ui_weak, login.role, login.perms, login.name);
                }
                Err(e) => {
                    update_ui_login_error(ui_weak, e);
                }
            }
        });
    });
}

/// Fluxo "esqueci a senha" (3 passos, online):
/// 1) `recovery-forgot` → POST /auth/forgot-password (envia código por e-mail);
/// 2) `recovery-verify` → POST /auth/verify-reset-code (valida o código, sem
///    consumir — só avança à tela de nova senha se der certo);
/// 3) `recovery-reset`  → POST /auth/reset-password (revalida código + troca).
///
/// Regras (§11): não vazamos se o e-mail existe (passo 1 sempre avança); a
/// autoridade é o backend (valida código/expiração/senha).
pub(crate) fn setup_password_recovery(
    ui: &MainWindow,
    handle: &tokio::runtime::Handle,
    server_url: String,
) {
    // Passo 1 — solicitar o código.
    {
        let ui_weak = ui.as_weak();
        let handle = handle.clone();
        let url = server_url.clone();
        ui.on_recovery_forgot(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            let email = ui.get_recovery_email().trim().to_string();
            if email.is_empty() {
                ui.set_recovery_status(SharedString::from("Informe seu e-mail."));
                return;
            }
            if !is_valid_email(&email) {
                ui.set_recovery_status(SharedString::from(
                    "Informe um e-mail válido (com @ e domínio).",
                ));
                return;
            }
            ui.set_recovery_busy(true);
            ui.set_recovery_status(SharedString::from("Enviando..."));
            let ui_weak = ui.as_weak();
            let url = url.clone();
            handle.spawn(async move {
                let res = HTTP_CLIENT
                    .post(format!("{url}/auth/forgot-password"))
                    .json(&serde_json::json!({ "email": email }))
                    .send()
                    .await;
                let online = matches!(&res, Ok(r) if r.status().is_success());
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = ui_weak.upgrade() else { return };
                    ui.set_recovery_busy(false);
                    if online {
                        // Não revela se o e-mail existe — sempre segue ao passo 2.
                        ui.set_recovery_step(2);
                        ui.set_recovery_status(SharedString::from(
                            "Se o e-mail estiver cadastrado, enviamos um código de 6 dígitos.",
                        ));
                    } else {
                        ui.set_recovery_status(SharedString::from("Sem conexão com o servidor."));
                    }
                });
            });
        });
    }
    // Passo 2 — validar o código (sem consumir). Só avança à tela de
    // nova senha se o backend confirmar (200).
    {
        let ui_weak = ui.as_weak();
        let handle = handle.clone();
        let url = server_url.clone();
        ui.on_recovery_verify(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            let email = ui.get_recovery_email().trim().to_string();
            let code = ui.get_recovery_code().trim().to_string();
            if code.is_empty() {
                ui.set_recovery_status(SharedString::from("Informe o código de 6 dígitos."));
                return;
            }
            ui.set_recovery_busy(true);
            ui.set_recovery_status(SharedString::from("Verificando..."));
            let ui_weak = ui.as_weak();
            let url = url.clone();
            handle.spawn(async move {
                let res = HTTP_CLIENT
                    .post(format!("{url}/auth/verify-reset-code"))
                    .json(&serde_json::json!({ "email": email, "code": code }))
                    .send()
                    .await;
                let outcome: Result<(), String> = match res {
                    Ok(r) if r.status().is_success() => Ok(()),
                    Ok(r) => {
                        let body = r.text().await.unwrap_or_default();
                        let msg = serde_json::from_str::<serde_json::Value>(&body)
                            .ok()
                            .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| "Código inválido ou expirado.".into());
                        Err(msg)
                    }
                    Err(_) => Err("Sem conexão com o servidor.".into()),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = ui_weak.upgrade() else { return };
                    ui.set_recovery_busy(false);
                    match outcome {
                        Ok(()) => {
                            // Código válido → libera a tela de nova senha.
                            ui.set_recovery_new_password(SharedString::default());
                            ui.set_recovery_status(SharedString::default());
                            ui.set_recovery_step(3);
                        }
                        Err(msg) => ui.set_recovery_status(SharedString::from(msg)),
                    }
                });
            });
        });
    }
    // Passo 3 — definir a nova senha (revalida o código + troca).
    {
        let ui_weak = ui.as_weak();
        let handle = handle.clone();
        let url = server_url;
        ui.on_recovery_reset(move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            let email = ui.get_recovery_email().trim().to_string();
            let code = ui.get_recovery_code().trim().to_string();
            let new_password = ui.get_recovery_new_password().to_string();
            if code.is_empty() || new_password.trim().is_empty() {
                ui.set_recovery_status(SharedString::from("Informe o código e a nova senha."));
                return;
            }
            ui.set_recovery_busy(true);
            ui.set_recovery_status(SharedString::from("Redefinindo..."));
            let ui_weak = ui.as_weak();
            let url = url.clone();
            handle.spawn(async move {
                let res = HTTP_CLIENT
                    .post(format!("{url}/auth/reset-password"))
                    .json(&serde_json::json!({
                        "email": email.clone(),
                        "code": code,
                        "new_password": new_password,
                    }))
                    .send()
                    .await;
                let outcome: Result<(), String> = match res {
                    Ok(r) if r.status().is_success() => Ok(()),
                    Ok(r) => {
                        let body = r.text().await.unwrap_or_default();
                        let msg = serde_json::from_str::<serde_json::Value>(&body)
                            .ok()
                            .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| "Não foi possível redefinir a senha.".into());
                        Err(msg)
                    }
                    Err(_) => Err("Sem conexão com o servidor.".into()),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = ui_weak.upgrade() else { return };
                    ui.set_recovery_busy(false);
                    match outcome {
                        Ok(()) => {
                            ui.set_recovery_step(0);
                            ui.set_recovery_code(SharedString::default());
                            ui.set_recovery_new_password(SharedString::default());
                            ui.set_recovery_status(SharedString::default());
                            ui.set_login_email(SharedString::from(email));
                            ui.set_login_status(SharedString::from(
                                "Senha redefinida! Entre com a nova senha.",
                            ));
                        }
                        Err(msg) => ui.set_recovery_status(SharedString::from(msg)),
                    }
                });
            });
        });
    }
}

/// Resultado de login bem-sucedido extraído da resposta do servidor.
pub(crate) struct LoginResult {
    pub(crate) token: String,
    pub(crate) company_id: Uuid,
    pub(crate) subdomain: String,
    pub(crate) company_name: String,
    pub(crate) role: UserRole,
    pub(crate) perms: Vec<String>,
    /// Nome do operador logado (rodapé da sidebar).
    pub(crate) name: String,
}

/// Aplica estado pós-login: atualiza company, grava JWT, notifica sync.
///
/// Regras aplicadas (AI_RULES.md §1, §7.4, §8, §11):
/// - Construção de entidade via service (nunca na UI — §1)
/// - Grava token no auth_token compartilhado com SyncWorker
/// - Dispara sync imediata via Notify (§7.4)
pub(crate) async fn apply_login(
    state: &DesktopState,
    auth_token: &RwLock<Option<String>>,
    notify: &Notify,
    server_company_id: Uuid,
    company_name: &str,
    subdomain: &str,
    token: String,
) {
    if server_company_id != state.company_id() {
        let old_id = state.company_id();

        if let Err(e) = state.company_service
            .register_remote(server_company_id, company_name.to_string(), subdomain.to_string())
            .await
        {
            tracing::error!("Failed to register remote company: {e}");
        }

        let _ = state.company_service.mark_synced(old_id).await;
        state.set_company_id(server_company_id);
        tracing::info!("company_id updated to {server_company_id}");
    }

    { *auth_token.write().await = Some(token.clone()); }

    // Persiste sessão no SQLite (§7.1 — offline-first)
    state.session.save_token(&token).await;
    state.session.save_company_id(server_company_id).await;
    // Subdomínio fica salvo (sobrevive ao logout) para identificar o
    // estabelecimento na próxima abertura do app.
    state.session.save_subdomain(subdomain).await;

    notify.notify_one();
}

/// Atualiza UI após login bem-sucedido.
///
/// Regras aplicadas (AI_RULES.md §11):
/// - `user-role` reflete o nível de acesso do operador (Admin/Funcionário).
/// - `refresh_business_hours` popula nome/endereço/logo no cabeçalho do menu.
pub(crate) fn update_ui_after_login(ui_weak: slint::Weak<MainWindow>, role: UserRole, perms: Vec<String>, name: String) {
    let _ = slint::invoke_from_event_loop(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        ui.set_logged_in(true);
        ui.set_user_role(SharedString::from(role.label_pt_br()));
        ui.set_user_name(SharedString::from(name));
        // Zera a foto do operador anterior; a do novo é carregada ao abrir
        // o perfil (GET /auth/me) e então cacheada.
        ui.set_profile_avatar(slint::Image::default());
        ui.set_profile_avatar_data(SharedString::default());
        ui.set_nav_perms(crate::nav_perms::nav_perms_from(role.is_admin(), role.is_super_admin(), &perms));
        // Abre a primeira aba que o operador pode acessar (evita cair
        // numa tela sem permissão após o login). Super admin → painel.
        ui.set_active_tab(SharedString::from(
            crate::nav_perms::first_accessible_tab(role.is_admin(), role.is_super_admin(), &perms),
        ));
        ui.set_login_status(SharedString::from("Login realizado!"));
        ui.set_status_message(SharedString::from("Conectado ao servidor"));
        ui.invoke_refresh_products();
        ui.invoke_refresh_business_hours();
        // Pré-carrega o status do caixa pra que o modal de bloqueio
        // do PDV (renderizado quando `cash-summary.open == false`)
        // já apareça/desapareça corretamente desde a primeira tela.
        ui.invoke_cash_refresh();
        // Dashboard é a landing page — popula KPIs/séries imediatamente
        // após login pra não mostrar tela em branco.
        ui.invoke_dashboard_refresh();
        ui.invoke_refresh_orders();
        // Financeiro (KPIs + fluxo de caixa) e Clientes (lista mestre)
        // — pré-carga garante que o usuário não veja tela vazia ao
        // navegar nessas abas antes do primeiro ciclo de sync.
        ui.invoke_finance_refresh();
        ui.invoke_refresh_customers();
        // Super admin: carrega o painel de plataforma (rotas /admin/*).
        if role.is_super_admin() {
            ui.invoke_admin_refresh();
        }
    });
}

/// Valida os campos do formulário de login antes de enviar.
///
/// Regras aplicadas (AI_RULES.md §8): responsabilidade única, sem lógica de negócio.
pub(crate) fn validate_login_fields(email: &str, password: &str) -> Option<&'static str> {
    let email = email.trim();
    match (email.is_empty(), password.is_empty()) {
        (true, true)  => Some("Informe o e-mail e a senha"),
        (true, false) => Some("Informe o e-mail"),
        (false, _) if !is_valid_email(email) => Some("Informe um e-mail válido (com @ e domínio)"),
        (false, true) => Some("Informe a senha"),
        (false, false) => None,
    }
}

/// Validação de formato de e-mail (§11 — o frontend só dá feedback; o backend
/// revalida). Exige `@` com parte local não-vazia e um domínio contendo `.`
/// (não nas bordas). Ex.: `a@b.com` ✓, `admin@demo` ✗, `x@y.` ✗.
pub(crate) fn is_valid_email(email: &str) -> bool {
    match email.trim().split_once('@') {
        Some((local, domain)) => {
            !local.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
        }
        None => false,
    }
}

/// Converte código HTTP de login em mensagem amigável em pt-BR.
pub(crate) fn map_login_error(status: StatusCode) -> String {
    match status.as_u16() {
        401 => "E-mail ou senha incorretos".into(),
        403 => "Acesso não autorizado".into(),
        404 => "Usuário não encontrado".into(),
        400 | 422 => "Dados inválidos. Verifique os campos.".into(),
        s if s >= 500 => "Erro no servidor. Tente novamente mais tarde.".into(),
        _ => format!("Erro inesperado (código {status})"),
    }
}

/// Atualiza UI após erro de login.
pub(crate) fn update_ui_login_error(ui_weak: slint::Weak<MainWindow>, error: String) {
    let _ = slint::invoke_from_event_loop(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        ui.set_login_status(SharedString::from(error));
    });
}

/// Executa login HTTP contra o servidor.
///
/// Regras aplicadas (AI_RULES.md §11):
/// - Servidor identifica a empresa automaticamente pelo email.
/// - Resposta carrega `user.role` para a UI decidir o rótulo (Admin/Funcionário).
pub(crate) async fn do_login(
    server_url: &str,
    email: &str,
    password: &str,
) -> Result<LoginResult, String> {
    #[derive(serde::Serialize)]
    struct Req { email: String, password: String }

    #[derive(serde::Deserialize)]
    struct Resp {
        token: String,
        user: RespUser,
        subdomain: String,
        company_name: String,
        #[serde(default)]
        perms: Vec<String>,
    }

    #[derive(serde::Deserialize)]
    struct RespUser { company_id: Uuid, role: UserRole, #[serde(default)] name: String }

    let url = format!("{server_url}/auth/login-desktop");
    let body = Req {
        email: email.to_string(),
        password: password.to_string(),
    };

    let resp = HTTP_CLIENT
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|_| "Sem conexão com o servidor".to_string())?;

    let status = resp.status();
    if !status.is_success() {
        return Err(map_login_error(status));
    }

    let data: Resp = resp.json().await
        .map_err(|_| "Erro ao processar resposta do servidor".to_string())?;

    Ok(LoginResult {
        token: data.token,
        company_id: data.user.company_id,
        subdomain: data.subdomain,
        company_name: data.company_name,
        role: data.user.role,
        perms: data.perms,
        name: data.user.name,
    })
}

#[cfg(test)]
mod tests {
    use super::{is_valid_email, validate_login_fields};

    #[test]
    fn email_exige_arroba_e_dominio_com_ponto() {
        assert!(is_valid_email("a@b.com"));
        assert!(is_valid_email("admin@demo.com.br"));
        assert!(!is_valid_email("admin@demo"));   // sem ponto no domínio
        assert!(!is_valid_email("admin.com"));    // sem @
        assert!(!is_valid_email("@demo.com"));    // sem parte local
        assert!(!is_valid_email("a@.com"));       // ponto na borda do domínio
        assert!(!is_valid_email("a@b."));         // ponto na borda
        assert!(!is_valid_email(""));
    }

    #[test]
    fn login_rejeita_email_invalido() {
        assert_eq!(validate_login_fields("", "x"), Some("Informe o e-mail"));
        assert!(validate_login_fields("admin@demo", "x").is_some()); // sem . → inválido
        assert_eq!(validate_login_fields("a@b.com", ""), Some("Informe a senha"));
        assert_eq!(validate_login_fields("a@b.com", "senha"), None);
    }
}
