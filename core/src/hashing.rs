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
