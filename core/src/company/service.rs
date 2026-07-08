use std::sync::Arc;

use uuid::Uuid;

use super::model::Company;
use super::repository::CompanyRepository;
use crate::error::CoreError;

/// Bag de atualização das Informações do Estabelecimento.
///
/// Regras aplicadas (AI_RULES.md §1, §8, §11):
/// - Struct evita explosão de parâmetros posicionais no `update_info`
///   (sempre uma fonte de bugs quando a UI passa argumentos errados).
/// - Campos opcionais refletem o modelo: NULL no banco = não informado.
/// - `name` é o único campo obrigatório (já era no schema anterior).
#[derive(Debug, Clone, Default)]
pub struct UpdateInfoInput {
    pub name: String,
    pub address: Option<String>,
    pub phone: Option<String>,
    pub whatsapp: Option<String>,
    pub email: Option<String>,
    pub instagram: Option<String>,
    pub document: Option<String>,
    pub neighborhood: Option<String>,
    pub zip_code: Option<String>,
    pub city: Option<String>,
    pub uf: Option<String>,
    pub logo_data: Option<String>,
    pub cover_data: Option<String>,
    pub products_per_page: i32,
    pub orders_per_page: i32,
    /// Fuso da loja (offset fixo de UTC em minutos). Ver `Company::utc_offset_minutes`.
    pub utc_offset_minutes: i32,
}

/// Service para o domínio Company.
///
/// Regras aplicadas (AI_RULES.md §1, §9, §11):
/// - service.rs contém a orquestração de regras de negócio
/// - Depende de repository via trait (inversão de dependência)
/// - Validar todos os dados de entrada no backend
///
/// Responsável por: resolver empresa por subdomínio,
/// validar existência, CRUD de empresas.
pub struct CompanyService {
    repo: Arc<dyn CompanyRepository>,
}

impl CompanyService {
    pub fn new(repo: Arc<dyn CompanyRepository>) -> Self {
        Self { repo }
    }

    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<Company>, CoreError> {
        self.repo.find_by_id(id).await
    }

    pub async fn find_by_subdomain(&self, subdomain: &str) -> Result<Option<Company>, CoreError> {
        self.repo.find_by_subdomain(subdomain).await
    }

    /// Resolve só o `company_id` pelo subdomínio (tenant middleware, §13).
    pub async fn find_id_by_subdomain(&self, subdomain: &str) -> Result<Option<Uuid>, CoreError> {
        self.repo.find_id_by_subdomain(subdomain).await
    }

    pub async fn find_all(&self) -> Result<Vec<Company>, CoreError> {
        self.repo.find_all().await
    }

    /// Cria uma empresa a partir de dados brutos.
    ///
    /// Valida entrada, constrói entidade, persiste e retorna.
    pub async fn create(
        &self,
        name: String,
        subdomain: String,
    ) -> Result<Company, CoreError> {
        if name.trim().is_empty() {
            return Err(CoreError::Validation("Company name is required".into()));
        }
        if subdomain.trim().is_empty() {
            return Err(CoreError::Validation("Company subdomain is required".into()));
        }
        let company = Company::new(name, subdomain);
        self.repo.create(&company).await?;
        Ok(company)
    }

    /// Atualiza uma empresa existente.
    ///
    /// Busca, valida, aplica alterações, atualiza timestamps e persiste.
    pub async fn update(
        &self,
        id: Uuid,
        name: String,
        subdomain: String,
    ) -> Result<Company, CoreError> {
        if name.trim().is_empty() {
            return Err(CoreError::Validation("Company name is required".into()));
        }
        if subdomain.trim().is_empty() {
            return Err(CoreError::Validation("Company subdomain is required".into()));
        }
        let mut company = self.repo.find_by_id(id).await?
            .ok_or_else(|| CoreError::NotFound("Company not found".into()))?;

        company.name = name;
        company.subdomain = subdomain;
        company.updated_at = chrono::Utc::now().naive_utc();
        company.synced = false;

        self.repo.update(&company).await?;
        Ok(company)
    }

    /// Remoção lógica (soft delete).
    pub async fn soft_delete(&self, id: Uuid) -> Result<(), CoreError> {
        self.repo.find_by_id(id).await?
            .ok_or_else(|| CoreError::NotFound("Company not found".into()))?;
        self.repo.soft_delete(id).await
    }

    /// Busca empresas ainda não sincronizadas (§7).
    pub async fn find_unsynced(&self) -> Result<Vec<Company>, CoreError> {
        self.repo.find_unsynced().await
    }

    /// Marca empresa como sincronizada (§7).
    pub async fn mark_synced(&self, id: Uuid) -> Result<(), CoreError> {
        self.repo.mark_synced(id).await
    }

    /// Atualiza informações do estabelecimento (todos os campos da tela
    /// "Informações do Estabelecimento"). Consolidados em [`UpdateInfoInput`]
    /// para evitar a lista posicional de parâmetros.
    ///
    /// Regras aplicadas (AI_RULES.md §1, §11):
    /// - `name` é obrigatório (validação no service, não na UI).
    /// - `products_per_page` é clamp em [1, 200] para evitar valores inúteis.
    /// - `document` (CPF/CNPJ) é apenas armazenado, sem validação no MVP.
    /// - `uf` é trimada e uppercase (defensivo) — mas vazias não são forçadas
    ///   a `Some("")` (vira `None`).
    pub async fn update_info(&self, id: Uuid, input: UpdateInfoInput) -> Result<Company, CoreError> {
        if input.name.trim().is_empty() {
            return Err(CoreError::Validation("Company name is required".into()));
        }
        let products_per_page = input.products_per_page.clamp(1, 200);
        let orders_per_page = input.orders_per_page.clamp(1, 200);
        // Offset plausível: UTC-12 (-720) a UTC+14 (+840).
        let utc_offset_minutes = input.utc_offset_minutes.clamp(-720, 840);
        let mut company = self.repo.find_by_id(id).await?
            .ok_or_else(|| CoreError::NotFound("Company not found".into()))?;
        company.name = input.name;
        company.address = input.address;
        company.phone = input.phone;
        company.whatsapp = input.whatsapp;
        company.email = input.email;
        company.instagram = input.instagram;
        company.document = input.document;
        company.neighborhood = input.neighborhood;
        company.zip_code = input.zip_code;
        company.city = input.city;
        company.uf = input.uf.map(|s| s.trim().to_uppercase()).filter(|s| !s.is_empty());
        company.logo_data = input.logo_data;
        company.cover_data = input.cover_data;
        company.products_per_page = products_per_page;
        company.orders_per_page = orders_per_page;
        company.utc_offset_minutes = utc_offset_minutes;
        company.updated_at = chrono::Utc::now().naive_utc();
        company.synced = false;
        self.repo.update(&company).await?;
        Ok(company)
    }

    /// Define o override de status do estabelecimento ("none", "open", "closed").
    ///
    /// Regras aplicadas (AI_RULES.md §1, §7):
    /// - Regra de negócio fica no service (§1)
    /// - Persiste no SQLite e marca synced = false para sync posterior (§7)
    pub async fn set_store_override(
        &self,
        id: Uuid,
        override_status: String,
    ) -> Result<Company, CoreError> {
        if !matches!(override_status.as_str(), "none" | "open" | "closed") {
            return Err(CoreError::Validation("override_status deve ser none, open ou closed".into()));
        }
        let mut company = self.repo.find_by_id(id).await?
            .ok_or_else(|| CoreError::NotFound("Company not found".into()))?;
        company.store_override = override_status;
        company.updated_at = chrono::Utc::now().naive_utc();
        company.synced = false;
        self.repo.update(&company).await?;
        Ok(company)
    }

    /// Registra uma empresa remota recebida do servidor.
    ///
    /// Regras aplicadas (AI_RULES.md §1, §7.7):
    /// - Construção de entidade fica na camada de service (nunca na UI)
    /// - Usa sync_upsert (last-write-wins) para persistir
    pub async fn register_remote(
        &self,
        id: Uuid,
        name: String,
        subdomain: String,
    ) -> Result<(), CoreError> {
        let epoch = chrono::DateTime::from_timestamp(0, 0)
            .map(|dt| dt.naive_utc())
            .unwrap_or_default();
        let company = Company {
            id,
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
            utc_offset_minutes: -180,
            created_at: epoch,
            updated_at: epoch,
            deleted_at: None,
            synced: false,
        };
        self.repo.sync_upsert(&company).await
    }

    /// Busca empresa atualizada após o timestamp (§7 — sync pull).
    pub async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
    ) -> Result<Vec<Company>, CoreError> {
        self.repo.find_updated_since(company_id, since).await
    }

    /// Upsert de sincronização (§7.7 — last-write-wins).
    ///
    /// Regras aplicadas (AI_RULES.md §7.7, §11):
    /// - Valida company.id contra o company_id autenticado
    /// - Marca synced = true antes de persistir
    /// - Repository resolve conflito via updated_at
    pub async fn sync_upsert(
        &self,
        company_id: Uuid,
        mut company: Company,
    ) -> Result<(), CoreError> {
        if company.id != company_id {
            return Err(CoreError::Validation("Company mismatch".into()));
        }
        match company.store_override.as_str() {
            "none" | "open" | "closed" => {}
            other => return Err(CoreError::Validation(
                format!("store_override inválido: '{other}' (esperado none|open|closed)")
            )),
        }
        company.synced = true;
        self.repo.sync_upsert(&company).await
    }
}
