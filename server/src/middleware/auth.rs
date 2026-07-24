use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use uuid::Uuid;

use crate::context::AppState;
use crate::error::ServerError;
use crate::jwt::{validate_token, Claims, ROLE_ADMIN, ROLE_EMPLOYEE, ROLE_SUPER_ADMIN};

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

    /// Aceita se o token concede QUALQUER uma das permissões (RBAC).
    /// Admin/SuperAdmin passam sempre. Usado quando uma mesma operação é
    /// legítima por caminhos diferentes — ex.: sincronizar um pedido serve
    /// tanto ao gestor (`orders.view`) quanto ao caixa que o criou (`pdv.view`).
    pub fn require_any_permission(&self, perms: &[&str]) -> Result<(), ServerError> {
        if self.0.role == ROLE_ADMIN || self.0.role == ROLE_SUPER_ADMIN {
            return Ok(());
        }
        if perms.iter().any(|perm| self.0.perms.iter().any(|p| p == perm)) {
            Ok(())
        } else {
            Err(ServerError::Forbidden("Permissão insuficiente".into()))
        }
    }

    /// Impede escalada de privilégio por delegação (§11): ao criar/editar uma
    /// Função, o chamador só pode conceder permissões que ELE MESMO possui.
    /// Sem isto, um gerente (Employee com `collaborators.edit`) montava uma
    /// Função com `finance.*`/`cash.*`/`subscription.edit`, atribuía a si e
    /// re-logava, ganhando acesso que nunca teve. Admin/SuperAdmin (acesso
    /// total) podem conceder qualquer permissão.
    pub fn require_can_grant(&self, perms: &[String]) -> Result<(), ServerError> {
        if self.0.role == ROLE_ADMIN || self.0.role == ROLE_SUPER_ADMIN {
            return Ok(());
        }
        if let Some(p) = perms.iter().find(|p| !self.0.perms.contains(p)) {
            return Err(ServerError::Forbidden(format!(
                "Você não pode conceder uma permissão que não possui: {p}"
            )));
        }
        Ok(())
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

        // Revogação de acesso (§11) para operadores do tenant (admin/employee),
        // numa única query indexada: `find_token_version` devolve `None` se o
        // usuário foi removido/desativado (deleted_at) → 401; e a versão de
        // credencial atual, que deve casar com o `tv` do token — se mudou
        // (role/permissões/senha alterados) o token é rejeitado na hora, sem
        // esperar o `exp`. `super_admin` (plataforma) e `customer` (outra
        // tabela) seguem outro ciclo e não passam por aqui.
        if claims.role == ROLE_ADMIN || claims.role == ROLE_EMPLOYEE {
            match state
                .auth_service
                .find_token_version(claims.company_id, claims.sub)
                .await?
            {
                None => {
                    return Err(ServerError::Jwt("Conta desativada ou removida".into()));
                }
                Some(v) if v != claims.tv => {
                    return Err(ServerError::Jwt(
                        "Credenciais alteradas; faça login novamente".into(),
                    ));
                }
                Some(_) => {}
            }
        }
        Ok(AuthClaims(claims))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jwt::{Claims, ROLE_ADMIN, ROLE_CUSTOMER, ROLE_EMPLOYEE, ROLE_SUPER_ADMIN};
    use uuid::Uuid;

    fn claims(role: &str, perms: &[&str]) -> AuthClaims {
        AuthClaims(Claims {
            sub: Uuid::new_v4(),
            company_id: Uuid::new_v4(),
            role: role.to_string(),
            perms: perms.iter().map(|s| s.to_string()).collect(),
            tv: 0,
            exp: 0,
        })
    }

    // ── Gate do painel do super admin (rotas /admin/*) ──────────────
    // Toda rota do painel chama `require_super_admin` → `verify_role`.
    // Estes testes travam a semântica desse gate: ele é a ÚNICA barreira
    // entre um operador comum e dados cross-tenant (AI_RULES §11).

    #[test]
    fn painel_admin_recusa_admin_de_loja() {
        // Admin é dono do SEU tenant, mas não da plataforma.
        let dono = claims(ROLE_ADMIN, &[]);
        assert!(dono.verify_role(ROLE_SUPER_ADMIN).is_err());
    }

    #[test]
    fn painel_admin_recusa_funcionario_e_cliente() {
        for role in [ROLE_EMPLOYEE, ROLE_CUSTOMER] {
            let c = claims(role, &["orders.view"]);
            assert!(
                c.verify_role(ROLE_SUPER_ADMIN).is_err(),
                "role {role} não pode acessar o painel"
            );
        }
    }

    #[test]
    fn painel_admin_aceita_super_admin() {
        let sa = claims(ROLE_SUPER_ADMIN, &[]);
        assert!(sa.verify_role(ROLE_SUPER_ADMIN).is_ok());
    }

    #[test]
    fn perms_nao_substituem_o_role_no_painel() {
        // Um funcionário com TODAS as permissões de loja continua fora do
        // painel: perms gateiam features do tenant, não a plataforma.
        let poderoso = claims(
            ROLE_EMPLOYEE,
            &["orders.edit", "finance.edit", "collaborators.edit", "subscription.edit"],
        );
        assert!(poderoso.verify_role(ROLE_SUPER_ADMIN).is_err());
    }

    #[test]
    fn gate_do_painel_e_cross_tenant_por_design() {
        // `verify_role` NÃO olha company_id: o super admin age sobre
        // qualquer empresa. É exceção documentada ao isolamento (§11) —
        // se um dia passar a checar tenant, o painel para de funcionar.
        let sa = claims(ROLE_SUPER_ADMIN, &[]);
        let outra_empresa = Uuid::new_v4();
        assert!(sa.verify_role(ROLE_SUPER_ADMIN).is_ok());
        assert!(
            sa.verify_company(outra_empresa).is_err(),
            "verify_company continua isolando — o painel usa verify_role de propósito"
        );
    }

    #[test]
    fn perfil_aceita_operadores_mas_nao_cliente() {
        // PUT /auth/profile usa verify_any_role com os 3 papéis de operador.
        let operadores = [ROLE_ADMIN, ROLE_EMPLOYEE, ROLE_SUPER_ADMIN];
        for role in operadores {
            assert!(claims(role, &[]).verify_any_role(&operadores).is_ok());
        }
        assert!(
            claims(ROLE_CUSTOMER, &[]).verify_any_role(&operadores).is_err(),
            "cliente final tem perfil próprio, não o de operador"
        );
    }

    #[test]
    fn funcionario_nao_concede_permissao_que_nao_tem() {
        let gerente = claims(ROLE_EMPLOYEE, &["collaborators.view", "collaborators.edit"]);
        // Tenta montar uma Função com finance.edit (que ele NÃO possui).
        let err = gerente.require_can_grant(&["finance.edit".to_string()]);
        assert!(err.is_err(), "escalada deveria ser bloqueada");
    }

    #[test]
    fn funcionario_concede_o_que_possui() {
        let gerente = claims(ROLE_EMPLOYEE, &["collaborators.view", "collaborators.edit", "orders.view"]);
        assert!(gerente
            .require_can_grant(&["orders.view".to_string(), "collaborators.view".to_string()])
            .is_ok());
    }

    #[test]
    fn admin_concede_qualquer_permissao() {
        let admin = claims(ROLE_ADMIN, &[]);
        assert!(admin
            .require_can_grant(&["finance.edit".to_string(), "cash.view".to_string()])
            .is_ok());
    }

    #[test]
    fn caixa_com_pdv_view_pode_sincronizar_pedido() {
        // Caixa sem orders.view mas com pdv.view: o push de pedido deve passar
        // (operar o PDV inclui criar/sincronizar a venda).
        let caixa = claims(ROLE_EMPLOYEE, &["pdv.view", "cash.view"]);
        assert!(caixa.require_any_permission(&["orders.view", "pdv.view"]).is_ok());
        // Sem nenhuma das duas → bloqueado.
        let estoquista = claims(ROLE_EMPLOYEE, &["stock.view"]);
        assert!(estoquista.require_any_permission(&["orders.view", "pdv.view"]).is_err());
    }
}
