//! Callbacks de autenticação e preferências de sessão. AI_RULES.md §1,
//! §8, §11: desktop é cliente burro — envia e-mail/senha, servidor
//! identifica a empresa e devolve o JWT.
//!
//! - `session`: tema (dark mode) e logout
//! - `login`: fluxo de login (validação, request, pós-login)

mod login;
mod profile;
mod session;

pub(crate) use login::{setup_login, setup_password_recovery};
pub(crate) use profile::setup_profile;
pub(crate) use session::{setup_dark_mode, setup_logout};
