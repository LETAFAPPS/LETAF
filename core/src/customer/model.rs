use serde::{Deserialize, Serialize};

use crate::entity::BaseFields;

/// Entidade Customer — cliente da empresa.
///
/// Regras aplicadas (AI_RULES.md §6):
/// - Campos base obrigatórios (UUID, company_id, timestamps, synced)
/// - Campos específicos do domínio
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Customer {
    #[serde(flatten)]
    pub base: BaseFields,
    pub name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub document: Option<String>,
    #[serde(skip_serializing)]
    pub password_hash: Option<String>,
    pub profile_picture: Option<String>,
    /// Observação interna do estabelecimento sobre o cliente
    /// (nunca exibida ao cliente final). `None` = sem observação.
    #[serde(default)]
    pub notes: Option<String>,
}

impl Customer {
    pub fn new(
        company_id: uuid::Uuid,
        name: String,
        email: Option<String>,
        phone: Option<String>,
        document: Option<String>,
    ) -> Self {
        Self {
            base: BaseFields::new(company_id),
            name,
            email,
            phone,
            document,
            password_hash: None,
            profile_picture: None,
            notes: None,
        }
    }

    /// Cria cliente com senha (registro via web).
    pub fn new_with_password(
        company_id: uuid::Uuid,
        name: String,
        email: String,
        phone: Option<String>,
        password_hash: String,
    ) -> Self {
        Self {
            base: BaseFields::new(company_id),
            name,
            email: Some(email),
            phone,
            document: None,
            password_hash: Some(password_hash),
            profile_picture: None,
            notes: None,
        }
    }
}
