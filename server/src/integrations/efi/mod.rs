//! Cliente HTTP da Efi Bank (PIX) com mTLS + OAuth.
//!
//! Regras aplicadas (AI_RULES.md §11):
//! - Toda chamada usa o `.p12` da empresa (mTLS obrigatório na Efi).
//! - OAuth token cacheado em memória até `expires_at - 60s` (renovação
//!   antecipada para evitar 401 em flight).
//! - Erros do gateway retornam `CoreError::Repository` com mensagem
//!   sanitizada (sem expor credenciais).
//!
//! Documentação: <https://dev.efipay.com.br/docs/api-pix/credenciais>

pub mod card;
pub mod client;
pub mod pix_auto;

pub use card::EfiCardClient;
pub use client::EfiClient;

/// Recorta o corpo de resposta de erro do gateway para o log (§11): respostas
/// de validação da Efi podem conter PII do pagador (nome/CPF). Mantém só o
/// início, suficiente para diagnóstico, sem despejar o corpo inteiro no log.
pub(crate) fn clip_body(body: &str) -> String {
    const MAX: usize = 200;
    let clipped: String = body.chars().take(MAX).collect();
    if body.chars().count() > MAX {
        format!("{clipped}… (truncado)")
    } else {
        clipped
    }
}
