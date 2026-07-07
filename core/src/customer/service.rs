use std::sync::Arc;

use uuid::Uuid;

use super::model::Customer;
use super::repository::CustomerRepository;
use crate::error::CoreError;

/// Service para o domínio Customer.
///
/// Regras aplicadas (AI_RULES.md §1, §9, §11):
/// - service.rs contém a orquestração de regras de negócio
/// - Depende de repository via trait (inversão de dependência)
/// - Validar todos os dados de entrada no backend
pub struct CustomerService {
    repo: Arc<dyn CustomerRepository>,
}

impl CustomerService {
    pub fn new(repo: Arc<dyn CustomerRepository>) -> Self {
        Self { repo }
    }

    pub async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<Customer>, CoreError> {
        self.repo.find_by_id(company_id, id).await
    }

    pub async fn find_all(&self, company_id: Uuid) -> Result<Vec<Customer>, CoreError> {
        self.repo.find_all(company_id).await
    }

    /// Cria um cliente a partir de dados brutos.
    ///
    /// Valida entrada, constrói entidade, persiste e retorna.
    pub async fn create(
        &self,
        company_id: Uuid,
        name: String,
        email: Option<String>,
        phone: Option<String>,
        document: Option<String>,
        notes: Option<String>,
    ) -> Result<Customer, CoreError> {
        if name.trim().is_empty() {
            return Err(CoreError::Validation("Customer name is required".into()));
        }
        let mut customer = Customer::new(company_id, name, email, phone, document);
        customer.notes = notes;
        self.repo.create(&customer).await?;
        Ok(customer)
    }

    /// Atualiza um cliente existente.
    ///
    /// Busca, valida, aplica alterações, atualiza timestamps e persiste.
    pub async fn update(
        &self,
        company_id: Uuid,
        id: Uuid,
        name: String,
        email: Option<String>,
        phone: Option<String>,
        document: Option<String>,
        notes: Option<String>,
    ) -> Result<Customer, CoreError> {
        if name.trim().is_empty() {
            return Err(CoreError::Validation("Customer name is required".into()));
        }
        let mut customer = self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("Customer not found".into()))?;

        customer.name = name;
        customer.email = email;
        customer.phone = phone;
        customer.document = document;
        customer.notes = notes;
        customer.base.updated_at = chrono::Utc::now().naive_utc();
        customer.base.synced = false;

        self.repo.update(&customer).await?;
        Ok(customer)
    }

    /// Registra um cliente final (web) com email e senha.
    ///
    /// Requer feature `password-hashing` (bcrypt não compila para WASM).
    #[cfg(feature = "password-hashing")]
    pub async fn register(
        &self,
        company_id: Uuid,
        name: String,
        email: String,
        phone: Option<String>,
        password: String,
    ) -> Result<Customer, CoreError> {
        if name.trim().is_empty() {
            return Err(CoreError::Validation("Customer name is required".into()));
        }
        if email.trim().is_empty() {
            return Err(CoreError::Validation("Customer email is required".into()));
        }
        if password.len() < 8 {
            return Err(CoreError::Validation("Password must be at least 8 characters".into()));
        }
        if self.repo.find_by_email(company_id, &email).await?.is_some() {
            return Err(CoreError::Validation("Email already registered".into()));
        }
        let hash = crate::hashing::hash_password(password).await?;
        let customer = Customer::new_with_password(company_id, name, email, phone, hash);
        self.repo.create(&customer).await?;
        Ok(customer)
    }

    /// Autentica um cliente final por email e senha.
    ///
    /// Requer feature `password-hashing`.
    #[cfg(feature = "password-hashing")]
    pub async fn authenticate(
        &self,
        company_id: Uuid,
        email: &str,
        password: &str,
    ) -> Result<Customer, CoreError> {
        let customer = self.repo.find_by_email(company_id, email).await?
            .ok_or_else(|| CoreError::Unauthorized("Invalid credentials".into()))?;
        let hash = customer.password_hash.as_deref()
            .ok_or_else(|| CoreError::Unauthorized("Customer has no password".into()))?;
        let valid = crate::hashing::verify_password(password.to_string(), hash.to_string()).await?;
        if !valid {
            return Err(CoreError::Unauthorized("Invalid credentials".into()));
        }
        Ok(customer)
    }

    /// Atualiza perfil do cliente final (web): nome, telefone e senha opcional.
    ///
    /// Regras aplicadas (AI_RULES.md §1, §11):
    /// - Verifica senha atual antes de permitir troca
    /// - Hash bcrypt para nova senha
    #[cfg(feature = "password-hashing")]
    pub async fn update_web_profile(
        &self,
        company_id: Uuid,
        customer_id: Uuid,
        name: String,
        phone: Option<String>,
        new_password: Option<String>,
        current_password: Option<String>,
        profile_picture: Option<String>,
    ) -> Result<Customer, CoreError> {
        if name.trim().is_empty() {
            return Err(CoreError::Validation("Name is required".into()));
        }
        let mut customer = self.repo.find_by_id(company_id, customer_id).await?
            .ok_or_else(|| CoreError::NotFound("Customer not found".into()))?;

        if let Some(new_pwd) = new_password {
            let cur_pwd = current_password
                .ok_or_else(|| CoreError::Validation("Current password required".into()))?;
            let hash = customer.password_hash.as_deref()
                .ok_or_else(|| CoreError::Unauthorized("No password set".into()))?;
            if !crate::hashing::verify_password(cur_pwd, hash.to_string()).await? {
                return Err(CoreError::Unauthorized("Current password incorrect".into()));
            }
            // Mesmo critério do `register` (linha 98) para evitar
            // política dupla — senha curta agora é rejeitada também
            // em mudanças de perfil.
            if new_pwd.len() < 8 {
                return Err(CoreError::Validation("Password must be at least 8 characters".into()));
            }
            // Mesmo custo do cadastro (BCRYPT_COST=13). Antes usava
            // DEFAULT_COST=12 — política de hash inconsistente para a
            // mesma entidade conforme o caminho (cadastro vs. troca).
            customer.password_hash = Some(crate::hashing::hash_password(new_pwd).await?);
        }

        customer.name            = name;
        customer.phone           = phone;
        if profile_picture.is_some() {
            customer.profile_picture = profile_picture;
        }
        customer.base.updated_at = chrono::Utc::now().naive_utc();
        customer.base.synced     = false;
        self.repo.update(&customer).await?;
        Ok(customer)
    }

    pub async fn find_by_email(&self, company_id: Uuid, email: &str) -> Result<Option<Customer>, CoreError> {
        self.repo.find_by_email(company_id, email).await
    }

    /// Remoção lógica (soft delete).
    pub async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("Customer not found".into()))?;
        self.repo.soft_delete(company_id, id).await
    }

    /// Busca clientes ainda não sincronizados (§7).
    pub async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<Customer>, CoreError> {
        self.repo.find_unsynced(company_id).await
    }

    /// Marca cliente como sincronizado (§7).
    pub async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        self.repo.mark_synced(company_id, id, updated_at).await
    }

    /// Busca clientes atualizados após o timestamp (§7 — sync pull).
    pub async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
    ) -> Result<Vec<Customer>, CoreError> {
        self.repo.find_updated_since(company_id, since).await
    }

    /// Página do pull por keyset `(updated_at, id)`.
    pub async fn find_updated_since_paged(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
        after_id: Uuid,
        limit: i64,
    ) -> Result<Vec<Customer>, CoreError> {
        self.repo.find_updated_since_paged(company_id, since, after_id, limit).await
    }

    /// Upsert de sincronização (§7.7 — last-write-wins).
    ///
    /// Regras aplicadas (AI_RULES.md §7.7, §11):
    /// - Valida company_id contra o tenant autenticado
    /// - Marca synced = true antes de persistir
    /// - Repository resolve conflito via updated_at
    pub async fn sync_upsert(
        &self,
        company_id: Uuid,
        mut customer: Customer,
    ) -> Result<(), CoreError> {
        if customer.base.company_id != company_id {
            return Err(CoreError::Validation("Company mismatch".into()));
        }
        customer.base.synced = true;
        self.repo.sync_upsert(&customer).await
    }
}
