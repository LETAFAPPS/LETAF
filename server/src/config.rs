use std::env;

/// Configuração do servidor.
///
/// Regras aplicadas (AI_RULES.md §4, §11):
/// - Backend usa axum + SQLx (PostgreSQL)
/// - Preparar autenticação (JWT)
/// - Segredos NUNCA hardcoded em produção
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub database_url: String,
    pub server_port: u16,
    pub jwt_secret: String,
    pub cors_origins: Vec<String>,
    /// `None` quando variáveis EFI_* não estão setadas — o servidor
    /// sobe normalmente, apenas os endpoints `/payments/*` retornam
    /// 503 indicando "gateway não configurado".
    pub efi: Option<EfiConfig>,
    /// Config da API Cobranças da Efi (cartão recorrente). Independente
    /// do PIX: base/credenciais próprias, sem mTLS. `None` desabilita
    /// os endpoints `/subscription/card*` (503).
    pub efi_card: Option<EfiCardConfig>,
    /// Diretório onde ficam o `manifest.json` (metadados da última versão
    /// do desktop) e os binários servidos por `/app/download/*`. Default
    /// `updates`. Permite publicar atualização sem recompilar o servidor.
    pub app_updates_dir: String,
    /// SMTP para envio de e-mail (recuperação de senha). `None` quando
    /// `SMTP_HOST` não está setado — o servidor sobe normalmente e o
    /// código de redefinição é apenas logado (modo dev).
    pub smtp: Option<SmtpConfig>,
}

/// Config SMTP para envio de e-mail transacional (recuperação de senha).
#[derive(Debug, Clone)]
pub struct SmtpConfig {
    pub host: String,
    /// 587 (STARTTLS) ou 465 (TLS implícito). Default 587.
    pub port: u16,
    pub username: String,
    pub password: String,
    /// Remetente (ex.: `LETAF <no-reply@seudominio.com>`).
    pub from: String,
}

impl SmtpConfig {
    fn from_env() -> Option<Self> {
        let host = env::var("SMTP_HOST").ok().filter(|s| !s.trim().is_empty())?;
        Some(Self {
            host,
            port: env::var("SMTP_PORT").ok().and_then(|s| s.parse().ok()).unwrap_or(587),
            username: env::var("SMTP_USER").unwrap_or_default(),
            password: env::var("SMTP_PASS").unwrap_or_default(),
            from: env::var("SMTP_FROM")
                .unwrap_or_else(|_| "LETAF <no-reply@letaf.app>".into()),
        })
    }
}

/// Credenciais da API **Cobranças** da Efi (cartão/assinatura).
///
/// Distinta da `EfiConfig` (PIX): base diferente, OAuth em
/// `/v1/authorize`, sem certificado `.p12`. Pode usar a mesma
/// aplicação Efi (mesmo client_id/secret) se ela tiver o escopo de
/// Cobranças habilitado — por isso o fallback para EFI_CLIENT_*.
#[derive(Debug, Clone)]
pub struct EfiCardConfig {
    /// `homologacao` ou `producao`.
    pub env: String,
    pub client_id: String,
    pub client_secret: String,
    /// Identificador de conta Efi (Payee Code) usado na tokenização.
    pub payee_code: String,
    /// URL pública que a Efi chama a cada cobrança (webhook). Precisa
    /// apontar para `POST /webhooks/efi` deste servidor.
    pub notification_url: String,
    /// Segredo `?hmac=` do webhook de CARTÃO (API Cobranças), análogo ao
    /// `EfiConfig::pix_webhook_hmac` do PIX. Lido de `EFI_CARD_WEBHOOK_HMAC`,
    /// com fallback para `EFI_PIX_WEBHOOK_HMAC` (compat.). Quando `Some`, o
    /// webhook de cartão exige a query `hmac` igual — sem depender da config
    /// do PIX, que pode nem estar habilitada (§11).
    pub webhook_hmac: Option<String>,
}

impl EfiCardConfig {
    pub fn base_url(&self) -> &'static str {
        if self.env == "producao" {
            "https://cobrancas.api.efipay.com.br"
        } else {
            "https://cobrancas-h.api.efipay.com.br"
        }
    }
}

#[derive(Debug, Clone)]
pub struct EfiConfig {
    /// `homologacao` ou `producao`.
    pub env: String,
    pub client_id: String,
    pub client_secret: String,
    pub p12_path: String,
    pub p12_password: String,
    pub pix_key: String,
    /// Segredo (`hmac`) que a Efi anexa à URL do webhook como `?hmac=`
    /// — mecanismo nativo deles, alternativa ao mTLS, p/ autenticar a
    /// origem. Quando `Some`, o webhook PIX exige a query `hmac` igual a
    /// este valor; quando `None`, não é exigido (compat. com deploys que
    /// só usam mTLS). Configurar em produção (§11).
    pub pix_webhook_hmac: Option<String>,
}

impl EfiConfig {
    pub fn base_url(&self) -> &'static str {
        if self.env == "producao" {
            "https://pix.api.efipay.com.br"
        } else {
            "https://pix-h.api.efipay.com.br"
        }
    }
}

impl AppConfig {
    /// Carrega config a partir de variáveis de ambiente.
    ///
    /// Regras aplicadas (AI_RULES.md §11):
    /// - Em release, exige `JWT_SECRET` definido (panic caso ausente).
    /// - Em debug usa fallback inseguro mas registra aviso explícito.
    /// - Avisa quando CORS está aberto (`*`).
    pub fn from_env() -> Self {
        let cors_origins: Vec<String> = env::var("CORS_ORIGINS")
            .unwrap_or_else(|_| "*".into())
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        if cors_origins.iter().any(|o| o == "*") {
            tracing::warn!("CORS_ORIGINS=* — restrinja em produção");
        }

        let jwt_secret = resolve_jwt_secret();

        Self {
            database_url: env::var("DATABASE_URL")
                .unwrap_or_else(|_| "postgres://localhost/letaf".into()),
            server_port: env::var("SERVER_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(3000),
            jwt_secret,
            cors_origins,
            efi: EfiConfig::from_env(),
            efi_card: EfiCardConfig::from_env(),
            app_updates_dir: env::var("APP_UPDATES_DIR")
                .unwrap_or_else(|_| "updates".into()),
            smtp: SmtpConfig::from_env(),
        }
    }
}

impl EfiCardConfig {
    /// Lê as variáveis da API Cobranças. `client_id`/`client_secret`
    /// caem para EFI_CLIENT_* quando os específicos não existem (mesma
    /// aplicação Efi com escopo de Cobranças). Retorna `None` quando
    /// faltar credencial, payee_code ou notification_url.
    fn from_env() -> Option<Self> {
        let client_id = non_empty("EFI_COBRANCAS_CLIENT_ID")
            .or_else(|| non_empty("EFI_CLIENT_ID"))?;
        let client_secret = non_empty("EFI_COBRANCAS_CLIENT_SECRET")
            .or_else(|| non_empty("EFI_CLIENT_SECRET"))?;
        let payee_code = non_empty("EFI_PAYEE_CODE")?;
        // Opcional: sem ela o cartão já habilita (tokenizar + assinatura
        // funcionam); só o webhook de cobranças recorrentes fica inativo
        // até apontar uma URL pública.
        let notification_url = non_empty("EFI_NOTIFICATION_URL").unwrap_or_default();
        let env = env::var("EFI_ENV").unwrap_or_else(|_| "homologacao".into());
        let webhook_hmac = non_empty("EFI_CARD_WEBHOOK_HMAC")
            .or_else(|| non_empty("EFI_PIX_WEBHOOK_HMAC"));
        Some(Self {
            env: if env == "producao" { env } else { "homologacao".into() },
            client_id,
            client_secret,
            payee_code,
            notification_url,
            webhook_hmac,
        })
    }
}

impl EfiConfig {
    /// Lê EFI_* do ambiente. Retorna `None` quando qualquer um dos
    /// campos obrigatórios estiver vazio — preferimos subir o server
    /// sem gateway a derrubar tudo por config incompleta (§11).
    fn from_env() -> Option<Self> {
        let client_id = non_empty("EFI_CLIENT_ID")?;
        let client_secret = non_empty("EFI_CLIENT_SECRET")?;
        let p12_path = non_empty("EFI_P12_PATH")?;
        let pix_key = non_empty("EFI_PIX_KEY")?;
        let env = env::var("EFI_ENV").unwrap_or_else(|_| "homologacao".into());
        let p12_password = env::var("EFI_P12_PASSWORD").unwrap_or_default();
        if env != "homologacao" && env != "producao" {
            tracing::warn!("EFI_ENV='{env}' inválido; usando 'homologacao'");
        }
        Some(Self {
            env: if env == "producao" { env } else { "homologacao".into() },
            client_id,
            client_secret,
            p12_path,
            p12_password,
            pix_key,
            pix_webhook_hmac: non_empty("EFI_PIX_WEBHOOK_HMAC"),
        })
    }
}

fn non_empty(key: &str) -> Option<String> {
    let v = env::var(key).ok()?;
    let v = v.trim();
    if v.is_empty() { None } else { Some(v.to_string()) }
}

/// Resolve `JWT_SECRET`, exigindo presença em builds de release.
///
/// Regras aplicadas (AI_RULES.md §8, §11):
/// - Função pequena com responsabilidade única.
/// - Falha cedo em produção quando o segredo não está configurado.
fn resolve_jwt_secret() -> String {
    match env::var("JWT_SECRET") {
        Ok(v) if !v.trim().is_empty() => v,
        _ => {
            if cfg!(debug_assertions) {
                tracing::error!(
                    "JWT_SECRET não definido — usando fallback APENAS para desenvolvimento"
                );
                "change-me-in-production".into()
            } else {
                panic!("JWT_SECRET é obrigatório em produção");
            }
        }
    }
}
