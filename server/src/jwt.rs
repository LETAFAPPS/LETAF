use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ServerError;

/// Roles suportadas no JWT.
///
/// Regras aplicadas (AI_RULES.md §11):
/// - 4 níveis de acesso:
///   * `super_admin`: cross-tenant (gestão de empresas, planos) — Fase 2
///   * `admin`:      dono do estabelecimento (gestão completa do tenant)
///   * `employee`:   colaborador (gestão com restrições futuras)
///   * `customer`:   cliente final do cardápio digital (web)
/// - Endpoints sensíveis aceitam apenas o role correto.
pub const ROLE_SUPER_ADMIN: &str = "super_admin";
pub const ROLE_ADMIN:       &str = "admin";
pub const ROLE_EMPLOYEE:    &str = "employee";
pub const ROLE_CUSTOMER:    &str = "customer";

/// Conjunto de roles considerados "operadores" — Admin ou Funcionário.
/// Usado em rotas que aceitam ambos os perfis do desktop.
pub const ROLES_OPERATORS: &[&str] = &[ROLE_ADMIN, ROLE_EMPLOYEE];

/// Claims do token JWT.
///
/// Regras aplicadas (AI_RULES.md §11):
/// - Preparar autenticação (JWT ou similar)
/// - Token contém user_id, company_id e role para isolamento multi-tenant
///   e separação de privilégios entre operadores e clientes finais
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: Uuid,
    pub company_id: Uuid,
    #[serde(default = "default_role")]
    pub role: String,
    /// Permissões efetivas (RBAC). Vazio para clientes e para tokens
    /// legados. Admin recebe o catálogo completo no login. Mudança de
    /// permissão exige re-login. Ver `core::permission`.
    #[serde(default)]
    pub perms: Vec<String>,
    pub exp: usize,
}

/// Default conservador para tokens antigos sem campo `role`.
/// Tokens legados (sem o campo) são tratados como `customer` (privilégio mínimo).
fn default_role() -> String {
    ROLE_CUSTOMER.to_string()
}

/// Cria um token JWT assinado.
///
/// Regras aplicadas (AI_RULES.md §11):
/// - `role` é obrigatório e identifica o tipo de principal
///   (`ROLE_USER` para operadores, `ROLE_CUSTOMER` para clientes finais)
pub fn create_token(
    subject_id: Uuid,
    company_id: Uuid,
    role: &str,
    perms: Vec<String>,
    secret: &str,
    expiration_hours: u64,
) -> Result<String, ServerError> {
    let expiration = chrono::Utc::now()
        .checked_add_signed(chrono::Duration::hours(expiration_hours as i64))
        .ok_or_else(|| ServerError::Jwt("Invalid expiration".into()))?
        .timestamp() as usize;

    let claims = Claims {
        sub: subject_id,
        company_id,
        role: role.to_string(),
        perms,
        exp: expiration,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| ServerError::Jwt(e.to_string()))
}

/// Valida e decodifica um token JWT.
///
/// Regras aplicadas (AI_RULES.md §11):
/// - Algoritmo HS256 explícito (default da lib, mas explicitado para auditoria)
/// - `validate_exp = true` rejeita tokens vencidos (também default, explicitado)
pub fn validate_token(token: &str, secret: &str) -> Result<Claims, ServerError> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map(|data| data.claims)
    .map_err(|e| ServerError::Jwt(e.to_string()))
}
