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

    /// Total de registros ativos da empresa (painel do super admin).
    pub async fn count_all(&self, company_id: Uuid) -> Result<i64, CoreError> {
        self.repo.count_all(company_id).await
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
            return Err(CoreError::Validation("Informe o nome do cliente".into()));
        }
        let mut customer = Customer::new(company_id, name, email, phone, document);
        customer.notes = notes;
        // Id determinístico pela chave natural (company_id, telefone): dois
        // terminais offline que cadastram o mesmo cliente por telefone
        // convergem para o mesmo id e o upsert dedupa (§7.7). Só quando há
        // telefone — sem ele, mantém o id aleatório de `Customer::new`.
        if let Some(digits) = phone_digits(&customer.phone) {
            customer.base.id = crate::deterministic_id::customer_by_phone(company_id, &digits);
        }
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
            return Err(CoreError::Validation("Informe o nome do cliente".into()));
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
            return Err(CoreError::Validation("Informe o nome do cliente".into()));
        }
        if email.trim().is_empty() {
            return Err(CoreError::Validation("Informe o e-mail do cliente".into()));
        }
        if password.len() < 8 {
            return Err(CoreError::Validation("A senha deve ter ao menos 8 caracteres".into()));
        }
        if self.repo.find_by_email(company_id, &email).await?.is_some() {
            return Err(CoreError::Validation("E-mail já cadastrado".into()));
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
        // Anti-enumeração (§11): mensagem ÚNICA para todos os casos de falha e
        // tempo equalizado (verify_dummy quando não há conta/senha) — não vazar
        // se o e-mail é cliente do tenant, nem marcar contas sem senha por
        // mensagem/latência distinta.
        let customer = match self.repo.find_by_email(company_id, email).await? {
            Some(c) => c,
            None => {
                crate::hashing::verify_dummy(password.to_string()).await;
                return Err(CoreError::Unauthorized("Credenciais inválidas".into()));
            }
        };
        let hash = match customer.password_hash.as_deref() {
            Some(h) => h.to_string(),
            None => {
                crate::hashing::verify_dummy(password.to_string()).await;
                return Err(CoreError::Unauthorized("Credenciais inválidas".into()));
            }
        };
        let valid = crate::hashing::verify_password(password.to_string(), hash).await?;
        if !valid {
            return Err(CoreError::Unauthorized("Credenciais inválidas".into()));
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
            return Err(CoreError::Validation("Informe o nome".into()));
        }
        let mut customer = self.repo.find_by_id(company_id, customer_id).await?
            .ok_or_else(|| CoreError::NotFound("Customer not found".into()))?;

        let mut password_changed = false;
        if let Some(new_pwd) = new_password {
            let cur_pwd = current_password
                .ok_or_else(|| CoreError::Validation("Informe a senha atual".into()))?;
            let hash = customer.password_hash.as_deref()
                .ok_or_else(|| CoreError::Unauthorized("Nenhuma senha cadastrada".into()))?;
            if !crate::hashing::verify_password(cur_pwd, hash.to_string()).await? {
                return Err(CoreError::Unauthorized("Senha atual incorreta".into()));
            }
            // Mesmo critério do `register` (linha 98) para evitar
            // política dupla — senha curta agora é rejeitada também
            // em mudanças de perfil.
            if new_pwd.len() < 8 {
                return Err(CoreError::Validation("A senha deve ter ao menos 8 caracteres".into()));
            }
            // Mesmo custo do cadastro (BCRYPT_COST=13). Antes usava
            // DEFAULT_COST=12 — política de hash inconsistente para a
            // mesma entidade conforme o caminho (cadastro vs. troca).
            customer.password_hash = Some(crate::hashing::hash_password(new_pwd).await?);
            password_changed = true;
        }

        customer.name            = name;
        customer.phone           = phone;
        if profile_picture.is_some() {
            customer.profile_picture = profile_picture;
        }
        customer.base.updated_at = chrono::Utc::now().naive_utc();
        customer.base.synced     = false;
        self.repo.update(&customer).await?;
        // Troca de senha revoga sessões ativas (§11): incrementa a versão de
        // credencial → o token antigo é rejeitado no próximo request. Inclui a
        // sessão atual (o web reautentica), fechando a janela de 72h.
        if password_changed {
            self.repo.bump_token_version(company_id, customer_id).await?;
        }
        Ok(customer)
    }

    /// Versão de credencial do cliente para o middleware validar o `tv` do JWT
    /// e para o login carimbar o token (§11 — revogação de sessão web).
    pub async fn find_token_version(&self, company_id: Uuid, id: Uuid) -> Result<Option<i32>, CoreError> {
        self.repo.find_token_version(company_id, id).await
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
            return Err(CoreError::Validation("Operação não permitida para esta empresa".into()));
        }
        customer.base.clamp_future_updated_at();
        customer.base.synced = true;
        self.repo.sync_upsert(&customer).await
    }
}

/// Reduz o telefone a apenas dígitos para servir de chave natural (id
/// determinístico). `(11) 99999-8888` e `11999998888` convergem. Retorna
/// `None` quando não há telefone ou ele não tem nenhum dígito — nesse caso
/// o cliente segue com id aleatório.
fn phone_digits(phone: &Option<String>) -> Option<String> {
    let digits: String = phone.as_deref()?.chars().filter(char::is_ascii_digit).collect();
    (!digits.is_empty()).then_some(digits)
}
