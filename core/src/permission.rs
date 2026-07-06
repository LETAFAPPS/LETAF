//! Catálogo de permissões do sistema (RBAC).
//!
//! Cada funcionalidade do operador tem permissão `view` (acessar a tela)
//! e, quando aplicável, `edit` (modificar/agir). As permissões são
//! atribuídas a uma [`crate::job_role::model::JobRole`] e valem para
//! Funcionários (`UserRole::Employee`). O `Admin` tem acesso total
//! (bypass) e não depende de permissões (AI_RULES §11 — o servidor é a
//! autoridade; a UI só reflete).
//!
//! A chave de permissão é `"<feature>.<action>"`, ex.: `"products.view"`,
//! `"products.edit"`. Este catálogo é a fonte única que alimenta a UI
//! (checkboxes), a validação (nunca confiar no frontend) e o "todas" do
//! Admin.

/// Ação de visualização (acessar/abrir a tela).
pub const VIEW: &str = "view";
/// Ação de edição (criar/alterar/remover/operar).
pub const EDIT: &str = "edit";

/// Catálogo: `(chave_da_feature, rótulo_pt_br, tem_edit?)`.
/// Telas só de leitura (painel, relatórios) não têm `edit`.
pub const FEATURES: &[(&str, &str, bool)] = &[
    ("dashboard", "Painel", false),
    ("reports", "Relatórios", false),
    ("pdv", "PDV", true),
    ("orders", "Pedidos", true),
    ("cash", "Caixa", true),
    ("finance", "Financeiro", true),
    ("products", "Produtos", true),
    ("stock", "Estoque", true),
    ("addons", "Adicionais", true),
    ("categories", "Categorias", true),
    ("banners", "Banners", true),
    ("coupons", "Cupons", true),
    ("customers", "Clientes", true),
    ("collaborators", "Colaboradores", true),
    ("subscription", "Assinatura", true),
];

/// Todas as chaves de permissão `"feature.action"` existentes — usada
/// para validar entrada e como conjunto implícito do Admin.
pub fn all() -> Vec<String> {
    let mut out = Vec::with_capacity(FEATURES.len() * 2);
    for (key, _, has_edit) in FEATURES {
        out.push(format!("{key}.{VIEW}"));
        if *has_edit {
            out.push(format!("{key}.{EDIT}"));
        }
    }
    out
}

/// `true` se `key` é uma permissão válida do catálogo (§11 — valida
/// entrada vinda do frontend antes de persistir numa função).
pub fn is_valid(key: &str) -> bool {
    let Some((feature, action)) = key.split_once('.') else {
        return false;
    };
    FEATURES.iter().any(|(k, _, has_edit)| {
        *k == feature && (action == VIEW || (action == EDIT && *has_edit))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_keys_are_valid_and_unique() {
        let keys = all();
        for k in &keys {
            assert!(is_valid(k), "{k} deveria ser válida");
        }
        let mut sorted = keys.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), keys.len(), "chaves duplicadas no catálogo");
    }

    #[test]
    fn rejects_unknown_or_malformed() {
        assert!(!is_valid("products"));          // sem ação
        assert!(!is_valid("dashboard.edit"));     // tela read-only não tem edit
        assert!(!is_valid("inexistente.view"));   // feature desconhecida
        assert!(!is_valid("products.delete"));    // ação inválida
        assert!(is_valid("products.edit"));
        assert!(is_valid("dashboard.view"));
    }
}
