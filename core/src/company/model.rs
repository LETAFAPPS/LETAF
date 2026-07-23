use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::NaiveDateTime;

/// Entidade Company — base do multi-tenant.
///
/// Regras aplicadas (AI_RULES.md §6):
/// - id: UUID (sem auto-incremento)
/// - created_at / updated_at: timestamps obrigatórios
/// - deleted_at: soft delete
/// - synced: controle de sincronização
///
/// Company não tem company_id próprio — ela É a empresa raiz.
/// Exceção documentada ao §6 (company_id): Company define o tenant,
/// portanto não referencia outro tenant.
/// O campo subdomain é usado para resolver a empresa via Host header.
fn default_store_override() -> String {
    "none".to_string()
}

fn default_products_per_page() -> i32 { 20 }

fn default_orders_per_page() -> i32 { 20 }
/// Offset padrão: -180 min = horário de Brasília (BRT, UTC-3).
fn default_utc_offset() -> i32 { -180 }
/// Empresas são ATIVAS por padrão (compatível com payloads sem o campo).
fn default_true() -> bool { true }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Company {
    pub id: Uuid,
    pub name: String,
    pub subdomain: String,
    #[serde(default = "default_store_override")]
    pub store_override: String,
    /// Endereço/rua + número juntos. Preservado por retrocompatibilidade
    /// com a versão anterior do schema; campos finos (bairro, cidade, UF,
    /// CEP) estão em colunas próprias adicionadas em [[migration_033]].
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    /// WhatsApp comercial — pode ser igual ao `phone` ou separado.
    #[serde(default)]
    pub whatsapp: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub instagram: Option<String>,
    /// CPF (11 dígitos) ou CNPJ (14 dígitos), armazenado como entrado
    /// pelo operador. Sem validação por enquanto — campo reservado para
    /// integrações fiscais futuras (emissão de nota etc.).
    #[serde(default)]
    pub document: Option<String>,
    #[serde(default)]
    pub neighborhood: Option<String>,
    #[serde(default)]
    pub zip_code: Option<String>,
    #[serde(default)]
    pub city: Option<String>,
    /// Sigla da UF (2 letras). Não normalizamos aqui.
    #[serde(default)]
    pub uf: Option<String>,
    #[serde(default)]
    pub logo_data: Option<String>,
    #[serde(default)]
    pub cover_data: Option<String>,
    /// Quantidade de produtos exibidos por página na grade.
    /// Configurável em Configurações; default 20.
    #[serde(default = "default_products_per_page")]
    pub products_per_page: i32,
    /// Quantidade de pedidos exibidos por página. Configurável separado
    /// de `products_per_page` porque cards de pedidos são maiores que
    /// cards de produtos (mais informações por linha).
    #[serde(default = "default_orders_per_page")]
    pub orders_per_page: i32,
    /// Fuso da loja como offset fixo de UTC em MINUTOS (ex.: -180 = BRT).
    /// Usado para validar janelas de horário (disponibilidade de produto e
    /// loja aberta) no backend a partir do `updated_at`/agora em UTC. Offset
    /// fixo é suficiente no Brasil (sem horário de verão). Default -180.
    #[serde(default = "default_utc_offset")]
    pub utc_offset_minutes: i32,
    /// Acesso do tenant. `false` = suspenso: o login é recusado (gate no
    /// server). É controle de PLATAFORMA (super admin) — server-authoritative:
    /// o sync do desktop nunca sobrescreve este campo.
    #[serde(default = "default_true")]
    pub active: bool,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub deleted_at: Option<NaiveDateTime>,
    pub synced: bool,
}

impl Company {
    pub fn new(name: String, subdomain: String) -> Self {
        let now = chrono::Utc::now().naive_utc();
        Self {
            id: Uuid::new_v4(),
            name,
            subdomain,
            store_override: "none".to_string(),
            address: None,
            phone: None,
            whatsapp: None,
            email: None,
            instagram: None,
            document: None,
            neighborhood: None,
            zip_code: None,
            city: None,
            uf: None,
            logo_data: None,
            cover_data: None,
            products_per_page: 20,
            orders_per_page: 20,
            utc_offset_minutes: default_utc_offset(),
            active: true,
            created_at: now,
            updated_at: now,
            deleted_at: None,
            synced: false,
        }
    }
}
