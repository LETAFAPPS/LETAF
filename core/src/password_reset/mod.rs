//! Recuperação de senha dos operadores (código de uso único por e-mail).
//!
//! Regras (AI_RULES.md §9, §10, §11): módulo `model`/`service`/`repository`;
//! acesso a dados só via repository; o código nunca é guardado em claro
//! (hash bcrypt) e expira. O envio por e-mail e a troca efetiva da senha
//! ficam no servidor (infra + `AuthService`).
pub mod model;
pub mod repository;
pub mod service;

use uuid::Uuid;

/// Gera um código numérico de 6 dígitos (`000000`–`999999`) usando a
/// aleatoriedade do UUID v4 (CSPRNG via `getrandom`) — evita nova dependência.
pub(crate) fn gen_code() -> String {
    let b = Uuid::new_v4().into_bytes();
    let n = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) % 1_000_000;
    format!("{n:06}")
}
