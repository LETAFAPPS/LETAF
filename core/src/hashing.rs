//! Hashing de senhas (bcrypt) fora do executor assíncrono.
//!
//! Regras (AI_RULES.md — Concorrência): bcrypt (cost 13) é CPU-bound e
//! leva centenas de ms; rodá-lo direto numa task async travaria uma
//! worker thread do Tokio, degradando TODAS as requisições daquela
//! thread sob carga de logins. Por isso o cálculo roda em
//! `spawn_blocking`. Feature-gated porque bcrypt não compila para WASM
//! (o web SSR usa o core sem `password-hashing`).

use crate::error::CoreError;

/// Custo de bcrypt para hash de senhas.
///
/// Valor 13 oferece margem extra sobre o `DEFAULT_COST` (12) contra
/// ataques com GPU (2025+). Política única de toda a base.
pub const BCRYPT_COST: u32 = 13;

/// Gera o hash de uma senha sem bloquear o executor (usa `spawn_blocking`).
pub async fn hash_password(password: String) -> Result<String, CoreError> {
    tokio::task::spawn_blocking(move || bcrypt::hash(password, BCRYPT_COST))
        .await
        .map_err(|e| CoreError::Repository(format!("hash task join: {e}")))?
        .map_err(|e| CoreError::Repository(format!("Hash error: {e}")))
}

/// Verifica uma senha contra o hash sem bloquear o executor.
pub async fn verify_password(password: String, hash: String) -> Result<bool, CoreError> {
    tokio::task::spawn_blocking(move || bcrypt::verify(password, &hash))
        .await
        .map_err(|e| CoreError::Repository(format!("verify task join: {e}")))?
        .map_err(|e| CoreError::Repository(format!("Hash verify error: {e}")))
}

/// Hash DUMMY de custo real (BCRYPT_COST), calculado UMA vez. Usado só para
/// equalizar o tempo de resposta do login (ver [`verify_dummy`]).
fn dummy_hash() -> &'static str {
    use std::sync::OnceLock;
    static H: OnceLock<String> = OnceLock::new();
    H.get_or_init(|| {
        bcrypt::hash("timing-equalizer", BCRYPT_COST).unwrap_or_default()
    })
}

/// Gasta ~o mesmo tempo de um `verify_password` real, verificando a senha
/// contra um hash dummy. Chamado quando o e-mail NÃO existe, para o login não
/// vazar a existência de contas por diferença de latência (§11 — enumeração
/// de usuários). Erros são ignorados de propósito: é só para consumir tempo.
pub async fn verify_dummy(password: String) {
    let _ = verify_password(password, dummy_hash().to_string()).await;
}
