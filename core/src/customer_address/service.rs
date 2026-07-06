use std::sync::Arc;

use uuid::Uuid;

use super::model::CustomerAddress;
use super::repository::CustomerAddressRepository;
use crate::error::CoreError;

/// Serviço de endereços de entrega do cliente.
///
/// Regras aplicadas (AI_RULES.md §1, §8, §11):
/// - Lógica de negócio no core, não na UI
/// - Validações antes de persistir
/// - Funções com responsabilidade única
pub struct CustomerAddressService {
    repo: Arc<dyn CustomerAddressRepository>,
}

impl CustomerAddressService {
    pub fn new(repo: Arc<dyn CustomerAddressRepository>) -> Self {
        Self { repo }
    }

    /// Lista endereços do cliente, isolado por company_id (§ isolamento).
    pub async fn list(
        &self,
        company_id: Uuid,
        customer_id: Uuid,
    ) -> Result<Vec<CustomerAddress>, CoreError> {
        self.repo.find_by_customer(company_id, customer_id).await
    }

    /// Lista todos os endereços da empresa de uma vez (para agrupar por
    /// cliente na UI sem N+1).
    pub async fn list_by_company(
        &self,
        company_id: Uuid,
    ) -> Result<Vec<CustomerAddress>, CoreError> {
        self.repo.find_by_company(company_id).await
    }

    /// Cria endereço com validações obrigatórias (§11).
    pub async fn create(
        &self,
        company_id: Uuid,
        customer_id: Uuid,
        label: String,
        custom_label: Option<String>,
        street: String,
        number: String,
        neighborhood: String,
        apartment: Option<String>,
    ) -> Result<CustomerAddress, CoreError> {
        validate_label(&label)?;
        require_field("Rua", &street)?;
        require_field("Número", &number)?;
        require_digits("Número", &number)?;
        require_field("Bairro", &neighborhood)?;

        let address = CustomerAddress::new(
            company_id, customer_id,
            label, custom_label,
            street, number, neighborhood, apartment,
        );
        self.repo.create(&address).await?;
        Ok(address)
    }

    /// Atualiza endereço existente com validações (§11).
    pub async fn update(
        &self,
        company_id: Uuid,
        id: Uuid,
        customer_id: Uuid,
        label: String,
        custom_label: Option<String>,
        street: String,
        number: String,
        neighborhood: String,
        apartment: Option<String>,
    ) -> Result<CustomerAddress, CoreError> {
        validate_label(&label)?;
        require_field("Rua", &street)?;
        require_field("Número", &number)?;
        require_digits("Número", &number)?;
        require_field("Bairro", &neighborhood)?;

        let mut address = self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("Endereço não encontrado".into()))?;
        if address.customer_id != customer_id {
            return Err(CoreError::Unauthorized("Endereço não pertence ao cliente".into()));
        }
        address.label        = label;
        address.custom_label = custom_label;
        address.street       = street;
        address.number       = number;
        address.neighborhood = neighborhood;
        address.apartment    = apartment;
        address.base.updated_at = chrono::Utc::now().naive_utc();
        address.base.synced     = false;
        self.repo.update(&address).await?;
        Ok(address)
    }

    /// Remove endereço logicamente (soft delete), verificando posse (§11).
    pub async fn soft_delete(
        &self,
        company_id: Uuid,
        id: Uuid,
        customer_id: Uuid,
    ) -> Result<(), CoreError> {
        self.repo.soft_delete(company_id, id, customer_id).await
    }

    // ── Sincronização offline-first (§7) ────────────────────────
    pub async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<CustomerAddress>, CoreError> {
        self.repo.find_unsynced(company_id).await
    }

    pub async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        self.repo.mark_synced(company_id, id, updated_at).await
    }

    pub async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
    ) -> Result<Vec<CustomerAddress>, CoreError> {
        self.repo.find_updated_since(company_id, since).await
    }

    pub async fn sync_upsert(
        &self,
        company_id: Uuid,
        mut address: CustomerAddress,
    ) -> Result<(), CoreError> {
        if address.base.company_id != company_id {
            return Err(CoreError::Validation("Company mismatch".into()));
        }
        address.base.synced = true;
        self.repo.sync_upsert(&address).await
    }
}

fn validate_label(label: &str) -> Result<(), CoreError> {
    match label {
        "Casa" | "Trabalho" | "Outros" => Ok(()),
        _ => Err(CoreError::Validation("Tipo de endereço inválido".into())),
    }
}

fn require_field(name: &str, value: &str) -> Result<(), CoreError> {
    if value.trim().is_empty() {
        Err(CoreError::Validation(format!("{name} é obrigatório")))
    } else {
        Ok(())
    }
}

/// Garante que o campo contém apenas dígitos ASCII. Usado em `number`
/// para barrar payloads com "123A" ou injeção via clientes que pulam
/// a máscara do frontend (AI_RULES.md §11).
fn require_digits(name: &str, value: &str) -> Result<(), CoreError> {
    if !value.chars().all(|c| c.is_ascii_digit()) {
        return Err(CoreError::Validation(format!("{name} deve conter apenas dígitos")));
    }
    Ok(())
}
