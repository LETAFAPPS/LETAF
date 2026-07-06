//! Verificação e aplicação de atualização do desktop (offline-first,
//! §7/§11).
//!
//! - Detecção: consulta `{server_url}/app/version` no boot e a cada 6h,
//!   compara a versão embutida (`CARGO_PKG_VERSION`) com o manifesto via
//!   semver e popula o estado do `UpdateModal`. O servidor é a autoridade
//!   sobre "última versão" e obrigatoriedade (§11); o desktop só compara.
//! - Aplicação (fase 2): baixa o binário, valida o `sha256` do manifesto,
//!   substitui o executável em execução (`self_replace`) e reinicia.
//!   Tudo em background — NUNCA bloqueia a UI; falha é silenciosa/exibida.

use std::collections::HashMap;
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};
use slint::SharedString;

use crate::{MainWindow, HTTP_CLIENT};

/// Intervalo entre checagens (além da checagem inicial no boot).
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Deserialize)]
struct UpdateManifest {
    latest: String,
    #[serde(default)]
    min_supported: Option<String>,
    #[serde(default)]
    notes: Option<String>,
    /// SO → nome do arquivo do binário no diretório de updates.
    #[serde(default)]
    files: HashMap<String, String>,
    /// SO → sha256 (hex) do binário, para verificar integridade no
    /// auto-update. Ausente = pula a verificação (best-effort).
    #[serde(default)]
    sha256: HashMap<String, String>,
}

/// Chave de SO usada em `files`/`sha256` do manifesto.
fn current_os() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

pub struct UpdateChecker {
    server_url: String,
    ui: slint::Weak<MainWindow>,
}

impl UpdateChecker {
    pub fn new(server_url: String, ui: slint::Weak<MainWindow>) -> Self {
        Self { server_url, ui }
    }

    pub async fn start(self) {
        tracing::info!("UpdateChecker iniciado (intervalo: {:?})", CHECK_INTERVAL);
        loop {
            self.check_once().await;
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    }

    async fn check_once(&self) {
        let Some(manifest) = self.fetch_manifest().await else { return };

        // Versões inválidas → ignora (não trava o app).
        let Ok(current) = semver::Version::parse(env!("CARGO_PKG_VERSION")) else { return };
        let latest = match semver::Version::parse(&manifest.latest) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("Versão 'latest' inválida no manifesto: {e}");
                return;
            }
        };
        if latest <= current {
            return; // já atualizado
        }

        let mandatory = manifest
            .min_supported
            .as_deref()
            .and_then(|s| semver::Version::parse(s).ok())
            .map(|min| current < min)
            .unwrap_or(false);

        let os = current_os();
        let url = manifest
            .files
            .get(os)
            .map(|file| format!("{}/app/download/{}", self.server_url, file))
            .unwrap_or_default();
        let sha = manifest.sha256.get(os).cloned().unwrap_or_default();
        let notes = manifest.notes.unwrap_or_default();
        let version = manifest.latest.clone();

        let ui = self.ui.clone();
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = ui.upgrade() else { return };
            ui.set_update_version(SharedString::from(version));
            ui.set_update_notes(SharedString::from(notes));
            ui.set_update_url(SharedString::from(url));
            ui.set_update_sha256(SharedString::from(sha));
            ui.set_update_mandatory(mandatory);
            ui.set_update_status(SharedString::from(""));
            ui.set_update_error(SharedString::from(""));
            ui.set_update_available(true);
        });
    }

    async fn fetch_manifest(&self) -> Option<UpdateManifest> {
        let url = format!("{}/app/version", self.server_url);
        let resp = HTTP_CLIENT.get(&url).timeout(Duration::from_secs(8)).send().await.ok()?;
        if !resp.status().is_success() || resp.status().as_u16() == 204 {
            return None;
        }
        resp.json::<UpdateManifest>().await.ok()
    }
}

/// Aplica a atualização: baixa o binário, valida o `sha256` (se houver),
/// substitui o executável em execução e reinicia o app. Roda numa task de
/// background; reporta status/erro na UI. Em sucesso, NÃO retorna (chama
/// `restart`).
pub async fn apply_update(url: String, expected_sha256: String, ui: slint::Weak<MainWindow>) {
    if url.is_empty() {
        set_error(&ui, "Sem binário para este sistema operacional".into());
        return;
    }
    set_status(&ui, "Baixando atualização…".into());
    let bytes = match download(&url).await {
        Ok(b) => b,
        Err(e) => return set_error(&ui, format!("Falha no download: {e}")),
    };

    if !expected_sha256.is_empty() {
        let got = sha256_hex(&bytes);
        if !got.eq_ignore_ascii_case(&expected_sha256) {
            return set_error(&ui, "Verificação de integridade (sha256) falhou".into());
        }
    }

    set_status(&ui, "Instalando…".into());
    // Escrita + troca do binário são bloqueantes → spawn_blocking.
    let replaced = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        let mut tmp = std::env::temp_dir();
        tmp.push(format!("letaf-update-{}.bin", std::process::id()));
        std::fs::write(&tmp, &bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
        }
        // Substitui o executável atual (self-replace cuida do lock no Windows).
        self_replace::self_replace(&tmp)?;
        let _ = std::fs::remove_file(&tmp);
        Ok(())
    })
    .await;

    match replaced {
        Ok(Ok(())) => {
            set_status(&ui, "Reiniciando…".into());
            restart();
        }
        Ok(Err(e)) => set_error(&ui, format!("Falha ao instalar: {e}")),
        Err(e) => set_error(&ui, format!("Falha ao instalar: {e}")),
    }
}

/// Reinicia o app: sobe um novo processo do executável (já atualizado) e
/// encerra o atual.
fn restart() -> ! {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe).spawn();
    }
    std::process::exit(0);
}

async fn download(url: &str) -> Result<Vec<u8>, String> {
    let resp = HTTP_CLIENT
        .get(url)
        .timeout(Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    resp.bytes().await.map(|b| b.to_vec()).map_err(|e| e.to_string())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn set_status(ui: &slint::Weak<MainWindow>, msg: String) {
    let ui = ui.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui.upgrade() {
            ui.set_update_error(SharedString::from(""));
            ui.set_update_status(SharedString::from(msg));
        }
    });
}

fn set_error(ui: &slint::Weak<MainWindow>, msg: String) {
    tracing::warn!("auto-update: {msg}");
    let ui = ui.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui.upgrade() {
            ui.set_update_status(SharedString::from(""));
            ui.set_update_error(SharedString::from(msg));
        }
    });
}

/// Abre a URL de download no navegador/SO (fallback manual quando o
/// auto-update falha). Best-effort.
pub fn open_url(url: &str) {
    if url.is_empty() {
        tracing::warn!("open_url: URL de atualização vazia");
        return;
    }
    let result = if cfg!(target_os = "windows") {
        std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(url).spawn()
    };
    if let Err(e) = result {
        tracing::warn!("Falha ao abrir URL de atualização '{url}': {e}");
    }
}
