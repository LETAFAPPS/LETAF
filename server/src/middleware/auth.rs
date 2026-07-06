use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use uuid::Uuid;

use crate::context::AppState;
use crate::error::ServerError;
use crate::jwt::{validate_token, Claims, ROLE_ADMIN, ROLE_SUPER_ADMIN};

/// Extractor axum que valida JWT do header Authorization.
///
/// Regras aplicadas (AI_RULES.md §11):
/// - Preparar autenticação (JWT ou similar)
/// - Validar dados de entrada no backend
///
/// Uso:
/// ```ignore
/// async fn handler(claims: AuthClaims) -> impl IntoResponse { ... }
/// ```
pub struct AuthClaims(pub Claims);

impl AuthClaims {
    /// Verifica que o company_id do token corresponde ao tenant.
    ///
    /// Regras aplicadas (AI_RULES.md §3, §11):
    /// - Nunca expor dados entre empresas
    /// - Nunca confiar em dados vindos do frontend
    pub fn verify_company(&self, expected: Uuid) -> Result<(), ServerError> {
        if self.0.company_id != expected {
            return Err(ServerError::Jwt("Token does not match tenant".into()));
        }
        Ok(())
    }

    /// Verifica que o role do token corresponde ao esperado.
    ///
    /// Regras aplicadas (AI_RULES.md §11):
    /// - Tokens de cliente final (`customer`) não podem acessar endpoints
    ///   de operador (`admin`/`employee`) e vice-versa.
    pub fn verify_role(&self, expected: &str) -> Result<(), ServerError> {
        if self.0.role != expected {
            return Err(ServerError::Jwt("Insufficient role".into()));
        }
        Ok(())
    }

    /// Verifica que o role do token está em uma lista de aceitáveis.
    ///
    /// Regras aplicadas (AI_RULES.md §11):
    /// - Útil para rotas operacionais que aceitam Admin ou Funcionário
    ///   (`ROLES_OPERATORS`) sem distinção a nível de endpoint.
    /// - Restrições granulares de Funcionário ficam para Fase 2 (capabilities).
    pub fn verify_any_role(&self, expected: &[&str]) -> Result<(), ServerError> {
        if expected.iter().any(|r| *r == self.0.role) {
            Ok(())
        } else {
            Err(ServerError::Jwt("Insufficient role".into()))
        }
    }

    /// Atalho: valida tenant + role na mesma chamada.
    pub fn verify(&self, company_id: Uuid, role: &str) -> Result<(), ServerError> {
        self.verify_company(company_id)?;
        self.verify_role(role)
    }

    /// Atalho: valida tenant + qualquer um dos roles aceitos.
    pub fn verify_any(&self, company_id: Uuid, roles: &[&str]) -> Result<(), ServerError> {
        self.verify_company(company_id)?;
        self.verify_any_role(roles)
    }

    /// Verifica que o token concede a permissão granular `perm` (RBAC).
    /// Admin/SuperAdmin têm acesso total (bypass). Funcionário precisa da
    /// permissão na lista do token (resolvida da sua Função no login).
    /// Ver `core::permission`.
    pub fn require_permission(&self, perm: &str) -> Result<(), ServerError> {
        if self.0.role == ROLE_ADMIN || self.0.role == ROLE_SUPER_ADMIN {
            return Ok(());
        }
        if self.0.perms.iter().any(|p| p == perm) {
            Ok(())
        } else {
            Err(ServerError::Forbidden("Permissão insuficiente".into()))
        }
    }
}

impl FromRequestParts<AppState> for AuthClaims {
    type Rejection = ServerError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| ServerError::Jwt("Missing authorization header".into()))?;

        let token = header
            .strip_prefix("Bearer ")
            .ok_or_else(|| ServerError::Jwt("Invalid authorization format".into()))?;

        let claims = validate_token(token, &state.config.jwt_secret)?;
        Ok(AuthClaims(claims))
    }
}
