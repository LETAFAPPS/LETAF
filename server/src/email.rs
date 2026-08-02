//! Envio de e-mail transacional (recuperação de senha) via SMTP.
//!
//! Se o SMTP não estiver configurado (`SMTP_HOST` ausente), o código é
//! apenas LOGADO (modo dev) e a função retorna `Ok` — o servidor sobe e o
//! fluxo funciona para testes sem depender de e-mail real.

use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::config::SmtpConfig;

/// Envia o código de redefinição de senha para `to`.
pub async fn send_reset_code(smtp: &Option<SmtpConfig>, to: &str, code: &str) -> Result<(), String> {
    let Some(cfg) = smtp else {
        // NUNCA logar o código: quem lê o log (journald, agregador) faria
        // takeover de qualquer conta pedindo o reset e lendo o código ali.
        // Em release, sem SMTP o reset FALHA — melhor negar o serviço que
        // vazar o segredo. Só em debug o código vai ao log, para facilitar o
        // desenvolvimento local sem servidor de e-mail.
        #[cfg(debug_assertions)]
        tracing::warn!("SMTP não configurado (DEBUG) — código de {to}: {code}");
        #[cfg(not(debug_assertions))]
        {
            let _ = (to, code);
            tracing::error!(
                "SMTP não configurado — não foi possível enviar o código de \
                 redefinição. Defina SMTP_HOST/PORT/USER/PASS/FROM."
            );
            return Err("Serviço de e-mail indisponível".into());
        }
        #[cfg(debug_assertions)]
        return Ok(());
    };

    let body = format!(
        "Olá,\n\nSeu código para redefinir a senha do LETAF é:\n\n    {code}\n\n\
         O código expira em 15 minutos. Se você não solicitou, ignore este e-mail.\n"
    );
    let email = Message::builder()
        .from(cfg.from.parse().map_err(|e| format!("remetente inválido: {e}"))?)
        .to(to.parse().map_err(|e| format!("destinatário inválido: {e}"))?)
        .subject("LETAF — Código de recuperação de senha")
        .header(ContentType::TEXT_PLAIN)
        .body(body)
        .map_err(|e| format!("falha ao montar e-mail: {e}"))?;

    // 587 → STARTTLS; 465 (ou outra) → TLS implícito.
    let builder = if cfg.port == 587 {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&cfg.host)
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::relay(&cfg.host)
    }
    .map_err(|e| format!("SMTP relay: {e}"))?;

    let mailer = builder
        .port(cfg.port)
        .credentials(Credentials::new(cfg.username.clone(), cfg.password.clone()))
        .build();

    mailer.send(email).await.map_err(|e| format!("envio SMTP: {e}"))?;
    Ok(())
}
