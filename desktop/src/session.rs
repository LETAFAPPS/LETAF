use chrono::NaiveDateTime;
use sqlx::SqlitePool;
use uuid::Uuid;

/// Repositório de sessão local (desktop).
///
/// Regras aplicadas (AI_RULES.md §5, §8, §10):
/// - Desktop usa SQLite
/// - Acesso ao banco somente via repository
/// - Funções pequenas com responsabilidade única
///
/// Persiste token JWT e company_id para manter sessão entre reinícios.
/// Não é entidade de domínio — não segue §6 (sem BaseFields/synced).
pub struct SessionStore {
    pool: SqlitePool,
}

const KEY_TOKEN: &str = "auth_token";
const KEY_COMPANY_ID: &str = "company_id";
const KEY_SUBDOMAIN: &str = "subdomain";
const KEY_LAST_PULL_AT: &str = "last_pull_at";
const KEY_DARK_MODE: &str = "dark_mode";
const KEY_REMEMBER_EMAIL: &str = "remember_email";
const KEY_REMEMBER_PASSWORD: &str = "remember_password";
const KEY_PERMS: &str = "nav_perms";
const KEY_IS_ADMIN: &str = "is_admin";
const KEY_IS_SUPER_ADMIN: &str = "is_super_admin";
const KEY_USER_NAME: &str = "user_name";
const TS_FMT: &str = "%Y-%m-%d %H:%M:%S%.6f";

impl SessionStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Carrega o token JWT salvo, se existir.
    pub async fn load_token(&self) -> Option<String> {
        self.get(KEY_TOKEN).await
    }

    /// Persiste o token JWT.
    pub async fn save_token(&self, token: &str) {
        self.set(KEY_TOKEN, token).await;
    }

    /// Carrega o company_id salvo, se existir.
    pub async fn load_company_id(&self) -> Option<Uuid> {
        self.get(KEY_COMPANY_ID).await
            .and_then(|s| Uuid::parse_str(&s).ok())
    }

    /// Persiste o company_id.
    pub async fn save_company_id(&self, id: Uuid) {
        self.set(KEY_COMPANY_ID, &id.to_string()).await;
    }

    /// Subdomínio do último login bem-sucedido. Mantido após logout
    /// (não é apagado por `clear`) para identificar o estabelecimento
    /// na próxima abertura do app.
    pub async fn load_subdomain(&self) -> Option<String> {
        self.get(KEY_SUBDOMAIN).await.filter(|s| !s.is_empty())
    }

    pub async fn save_subdomain(&self, subdomain: &str) {
        self.set(KEY_SUBDOMAIN, subdomain).await;
    }

    /// Carrega o timestamp do último pull bem-sucedido.
    pub async fn load_last_pull_at(&self) -> Option<NaiveDateTime> {
        self.get(KEY_LAST_PULL_AT).await
            .and_then(|s| NaiveDateTime::parse_from_str(&s, TS_FMT).ok())
    }

    /// Persiste o timestamp do último pull bem-sucedido.
    pub async fn save_last_pull_at(&self, ts: NaiveDateTime) {
        self.set(KEY_LAST_PULL_AT, &ts.format(TS_FMT).to_string()).await;
    }

    /// Carrega a preferência de tema escuro (false = claro por padrão).
    pub async fn load_dark_mode(&self) -> bool {
        self.get(KEY_DARK_MODE).await
            .map(|s| s == "true")
            .unwrap_or(false)
    }

    /// Persiste a preferência de tema escuro.
    pub async fn save_dark_mode(&self, dark: bool) {
        self.set(KEY_DARK_MODE, if dark { "true" } else { "false" }).await;
    }

    /// Carrega o email salvo para "Lembrar acesso", se existir.
    pub async fn load_remember_email(&self) -> Option<String> {
        self.get(KEY_REMEMBER_EMAIL).await
    }

    /// Persiste APENAS o email para "Lembrar acesso". A senha NUNCA é
    /// gravada (segurança: o `letaf.db` fica em disco; senha em texto
    /// puro era exposição direta). Também remove qualquer senha gravada
    /// por versões antigas do app.
    pub async fn save_remember_me(&self, email: &str) {
        self.set(KEY_REMEMBER_EMAIL, email).await;
        if let Err(e) = sqlx::query("DELETE FROM sessions WHERE key = ?1")
            .bind(KEY_REMEMBER_PASSWORD)
            .execute(&self.pool)
            .await
        {
            tracing::warn!("Falha ao limpar senha legada do remember-me: {e}");
        }
    }

    /// Remove as credenciais salvas de "Lembrar acesso".
    pub async fn clear_remember_me(&self) {
        let result = sqlx::query(
            "DELETE FROM sessions WHERE key IN (?1, ?2)",
        )
        .bind(KEY_REMEMBER_EMAIL)
        .bind(KEY_REMEMBER_PASSWORD)
        .execute(&self.pool)
        .await;
        if let Err(e) = result {
            tracing::error!("Failed to clear remember-me: {e}");
        }
    }

    /// Remove apenas as chaves de autenticação (token + company_id).
    ///
    /// Usado quando o servidor rejeita o token (ex.: company_id divergente
    /// após reset do banco, token expirado, usuário removido). Força o
    /// próximo boot a pedir re-login.
    ///
    /// Preserva `dark_mode` e `last_pull_at` intencionalmente:
    /// - preferência de tema não deve ser perdida ao deslogar
    /// - timestamp de pull evita re-download desnecessário de dados
    pub async fn clear(&self) {
        let result = sqlx::query(
            "DELETE FROM sessions WHERE key IN (?1, ?2)",
        )
        .bind(KEY_TOKEN)
        .bind(KEY_COMPANY_ID)
        .execute(&self.pool)
        .await;
        if let Err(e) = result {
            tracing::error!("Failed to clear auth session: {e}");
        }
    }

    /// Persiste as permissões efetivas do operador (RBAC §11) para que a
    /// gating da navegação sobreviva a um restart offline (sem servidor).
    pub async fn save_perms(&self, is_admin: bool, is_super_admin: bool, perms: &[String]) {
        self.set(KEY_IS_ADMIN, if is_admin { "true" } else { "false" }).await;
        self.set(KEY_IS_SUPER_ADMIN, if is_super_admin { "true" } else { "false" }).await;
        let json = serde_json::to_string(perms).unwrap_or_else(|_| "[]".into());
        self.set(KEY_PERMS, &json).await;
    }

    /// Persiste o nome do operador logado (rodapé da sidebar). Sobrevive
    /// a restart offline.
    pub async fn save_user_name(&self, name: &str) {
        self.set(KEY_USER_NAME, name).await;
    }

    /// Carrega o nome do operador logado, se houver.
    pub async fn load_user_name(&self) -> Option<String> {
        self.get(KEY_USER_NAME).await.filter(|s| !s.is_empty())
    }

    /// Carrega `(is_admin, is_super_admin, perms)` salvos. Default: sem
    /// permissões.
    pub async fn load_perms(&self) -> (bool, bool, Vec<String>) {
        let is_admin = self.get(KEY_IS_ADMIN).await.map(|v| v == "true").unwrap_or(false);
        let is_super_admin = self
            .get(KEY_IS_SUPER_ADMIN)
            .await
            .map(|v| v == "true")
            .unwrap_or(false);
        let perms = self
            .get(KEY_PERMS)
            .await
            .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
            .unwrap_or_default();
        (is_admin, is_super_admin, perms)
    }

    /// Lê um valor da tabela sessions.
    async fn get(&self, key: &str) -> Option<String> {
        sqlx::query_scalar::<_, String>("SELECT value FROM sessions WHERE key = ?1")
            .bind(key)
            .fetch_optional(&self.pool)
            .await
            .ok()
            .flatten()
    }

    /// Insere ou atualiza um valor na tabela sessions.
    async fn set(&self, key: &str, value: &str) {
        let result = sqlx::query(
            "INSERT INTO sessions (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await;

        if let Err(e) = result {
            tracing::error!("Failed to save session key '{key}': {e}");
        }
    }
}
