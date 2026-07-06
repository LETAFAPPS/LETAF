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
