use std::sync::Arc;

use uuid::Uuid;

use super::model::{User, UserRole};
use super::repository::UserRepository;
use crate::error::CoreError;

/// Re-export do custo de bcrypt (fonte única em `crate::hashing`).
#[cfg(feature = "password-hashing")]
pub use crate::hashing::BCRYPT_COST;

/// Service para autenticação.
///
/// Regras aplicadas (AI_RULES.md §1, §9, §11):
/// - service.rs contém a orquestração de regras de negócio
/// - Depende de repository via trait (inversão de dependência)
/// - Validar todos os dados de entrada no backend
/// - Preparar autenticação (JWT ou similar)
///
/// Responsável por: login, CRUD de usuários, hash de senha.
pub struct AuthService {
    repo: Arc<dyn UserRepository>,
}

impl AuthService {
    pub fn new(repo: Arc<dyn UserRepository>) -> Self {
        Self { repo }
    }

    pub async fn find_by_id(&self, company_id: Uuid, id: Uuid) -> Result<Option<User>, CoreError> {
        self.repo.find_by_id(company_id, id).await
    }

    /// Versão de credencial atual (RBAC §11 — revogação de JWT). `None` =
    /// usuário inexistente/desativado.
    pub async fn find_token_version(
        &self,
        company_id: Uuid,
        id: Uuid,
    ) -> Result<Option<i32>, CoreError> {
        self.repo.find_token_version(company_id, id).await
    }

    /// Incrementa a versão de credencial (invalida tokens emitidos antes).
    pub async fn bump_token_version(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        self.repo.bump_token_version(company_id, id).await
    }

    /// Revoga tokens de todos os usuários de uma função (permissões mudaram).
    pub async fn bump_token_version_by_job_role(
        &self,
        company_id: Uuid,
        job_role_id: Uuid,
    ) -> Result<(), CoreError> {
        self.repo.bump_token_version_by_job_role(company_id, job_role_id).await
    }

    pub async fn find_by_email(&self, company_id: Uuid, email: &str) -> Result<Option<User>, CoreError> {
        self.repo.find_by_email(company_id, email).await
    }

    /// Redefine a senha de um usuário identificado pelo e-mail (global) —
    /// usado no fluxo "esqueci a senha" após validar o código. Requer
    /// feature `password-hashing`.
    #[cfg(feature = "password-hashing")]
    pub async fn reset_password_global(&self, email: &str, new_password: &str) -> Result<(), CoreError> {
        if new_password.trim().chars().count() < 6 {
            return Err(CoreError::Validation("A senha deve ter ao menos 6 caracteres".into()));
        }
        let mut user = self.repo.find_by_email_global(email).await?
            .ok_or_else(|| CoreError::NotFound("Usuário não encontrado".into()))?;
        user.password_hash = crate::hashing::hash_password(new_password.to_string()).await?;
        user.base.updated_at = chrono::Utc::now().naive_utc();
        user.base.synced = false;
        self.repo.update(&user).await?;
        // Troca de senha invalida tokens emitidos antes (RBAC §11 — no-op no
        // desktop, que não versiona credencial).
        self.repo.bump_token_version(user.base.company_id, user.base.id).await?;
        Ok(())
    }

    /// Busca um usuário por email em TODO o sistema (cross-tenant).
    /// Retorna `Err(Validation)` se o email existir em mais de uma empresa
    /// (o login do desktop é global por email — precisa ser único). Usado
    /// para validar o email do super admin no painel de administrador.
    pub async fn find_by_email_global(&self, email: &str) -> Result<Option<User>, CoreError> {
        self.repo.find_by_email_global(email).await
    }

    pub async fn find_all(&self, company_id: Uuid) -> Result<Vec<User>, CoreError> {
        self.repo.find_all(company_id).await
    }

    /// Cria um usuário a partir de dados brutos.
    ///
    /// Valida entrada, faz hash da senha, constrói entidade, persiste e retorna.
    /// Requer feature `password-hashing` (bcrypt não compila para WASM).
    ///
    /// Regras aplicadas (AI_RULES.md §11):
    /// - `role` define o nível de acesso (Admin / Employee / SuperAdmin).
    #[cfg(feature = "password-hashing")]
    pub async fn create(
        &self,
        company_id: Uuid,
        email: String,
        password: String,
        name: String,
        role: UserRole,
    ) -> Result<User, CoreError> {
        if email.trim().is_empty() {
            return Err(CoreError::Validation("Informe o e-mail do usuário".into()));
        }
        if name.trim().is_empty() {
            return Err(CoreError::Validation("Informe o nome do usuário".into()));
        }
        if password.trim().is_empty() {
            return Err(CoreError::Validation("Informe a senha".into()));
        }
        // E-mail é identificador GLOBAL para o login do desktop
        // (`find_by_email_global`, que recusa duplicado). Não pode já existir
        // em outra empresa — senão o dono de outra loja fica sem conseguir
        // logar. Mesmo critério de `create_employee`.
        match self.repo.find_by_email_global(&email).await {
            Ok(Some(u)) if u.base.company_id != company_id && u.base.deleted_at.is_none() => {
                return Err(CoreError::Validation(
                    "E-mail já cadastrado em outra empresa".into(),
                ));
            }
            Err(_) => {
                return Err(CoreError::Validation(
                    "E-mail já cadastrado em outra empresa".into(),
                ));
            }
            _ => {}
        }
        if self.repo.find_by_email(company_id, &email).await?.is_some() {
            return Err(CoreError::Validation("E-mail já cadastrado".into()));
        }
        let password_hash = crate::hashing::hash_password(password).await?;
        let mut user = User::new(company_id, email, password_hash, name, role);
        // Id determinístico pela chave natural (company_id, email): dois
        // terminais offline que cadastram o mesmo e-mail convergem ao mesmo
        // id e o upsert dedupa, em vez de envenenar a fila na UNIQUE.
        user.base.id = crate::deterministic_id::user(company_id, &user.email);
        self.repo.create(&user).await?;
        Ok(user)
    }

    /// Cria um FUNCIONÁRIO (`Employee`) com uma Função (job_role)
    /// atribuída — usado pelo cadastro de colaboradores (RBAC).
    /// As permissões efetivas vêm da Função no login.
    #[cfg(feature = "password-hashing")]
    pub async fn create_employee(
        &self,
        company_id: Uuid,
        email: String,
        password: String,
        name: String,
        job_role_id: Option<Uuid>,
    ) -> Result<User, CoreError> {
        if email.trim().is_empty() {
            return Err(CoreError::Validation("E-mail do funcionário é obrigatório".into()));
        }
        if name.trim().is_empty() {
            return Err(CoreError::Validation("Nome do funcionário é obrigatório".into()));
        }
        if password.trim().is_empty() {
            return Err(CoreError::Validation("Senha é obrigatória".into()));
        }
        // Isolamento entre empresas: o e-mail NÃO pode já pertencer a OUTRA
        // empresa. O login do desktop resolve o usuário pelo e-mail em escopo
        // GLOBAL (`find_by_email_global`), e ele RECUSA quando o mesmo e-mail
        // existe em mais de uma empresa. Sem esta checagem, a loja A cadastrava
        // o e-mail do dono da loja B e travava o login (e a recuperação de
        // senha) dele para sempre — DoS cross-tenant. `Err` = já está em duas
        // empresas; barra também.
        match self.repo.find_by_email_global(&email).await {
            Ok(Some(u)) if u.base.company_id != company_id && u.base.deleted_at.is_none() => {
                return Err(CoreError::Validation(
                    "E-mail já cadastrado em outra empresa".into(),
                ));
            }
            Err(_) => {
                return Err(CoreError::Validation(
                    "E-mail já cadastrado em outra empresa".into(),
                ));
            }
            _ => {}
        }
        // Dentro do PRÓPRIO tenant: e-mail de funcionário ATIVO é duplicado
        // (erro); de funcionário EXCLUÍDO é reaproveitado (reativação) — senão
        // o INSERT violaria a UNIQUE (company_id, email).
        let existing = self.repo.find_by_email_any(company_id, &email).await?;
        if matches!(&existing, Some(u) if u.base.deleted_at.is_none()) {
            return Err(CoreError::Validation("E-mail já cadastrado".into()));
        }
        let password_hash = crate::hashing::hash_password(password).await?;
        match existing {
            Some(mut user) => {
                user.name = name;
                user.password_hash = password_hash;
                user.role = UserRole::Employee;
                user.job_role_id = job_role_id;
                user.base.deleted_at = None;
                user.base.updated_at = chrono::Utc::now().naive_utc();
                user.base.synced = false;
                self.repo.update(&user).await?;
                Ok(user)
            }
            None => {
                let mut user = User::new(company_id, email, password_hash, name, UserRole::Employee);
                user.job_role_id = job_role_id;
                // Id determinístico pela chave natural (company_id, email) —
                // convergência offline; mesmo motivo do `create`.
                user.base.id = crate::deterministic_id::user(company_id, &user.email);
                self.repo.create(&user).await?;
                Ok(user)
            }
        }
    }

    /// Atualiza um funcionário: nome, Função e, opcionalmente, a senha
    /// (quando `new_password` é `Some` e não-vazia). Não permite trocar
    /// o e-mail (login estável).
    #[cfg(feature = "password-hashing")]
    pub async fn update_employee(
        &self,
        company_id: Uuid,
        id: Uuid,
        name: String,
        job_role_id: Option<Uuid>,
        new_password: Option<String>,
    ) -> Result<User, CoreError> {
        if name.trim().is_empty() {
            return Err(CoreError::Validation("Nome do funcionário é obrigatório".into()));
        }
        let mut user = self
            .repo
            .find_by_id(company_id, id)
            .await?
            .ok_or_else(|| CoreError::NotFound("Funcionário não encontrado".into()))?;
        // §11: a tela de Colaboradores gerencia APENAS funcionários. Sem esta
        // guarda, um usuário com `collaborators.edit` (gerente) sobrescreveria a
        // senha/Função de um Admin/SuperAdmin e assumiria a conta (escalada
        // vertical). O role do alvo é autoridade do servidor.
        if user.role != UserRole::Employee {
            return Err(CoreError::Unauthorized(
                "Só é possível editar funcionários por aqui".into(),
            ));
        }
        // Mudança de função (permissões) ou de senha revoga tokens antigos.
        let role_changed = user.job_role_id != job_role_id;
        user.name = name;
        user.job_role_id = job_role_id;
        let mut pw_changed = false;
        if let Some(pw) = new_password.filter(|p| !p.trim().is_empty()) {
            user.password_hash = crate::hashing::hash_password(pw).await?;
            pw_changed = true;
        }
        user.base.updated_at = chrono::Utc::now().naive_utc();
        user.base.synced = false;
        self.repo.update(&user).await?;
        if role_changed || pw_changed {
            self.repo.bump_token_version(company_id, id).await?;
        }
        Ok(user)
    }

    /// Atualiza um usuário existente.
    ///
    /// Busca, valida, aplica alterações, atualiza timestamps e persiste.
    pub async fn update(
        &self,
        company_id: Uuid,
        id: Uuid,
        email: String,
        name: String,
    ) -> Result<User, CoreError> {
        if email.trim().is_empty() {
            return Err(CoreError::Validation("Informe o e-mail do usuário".into()));
        }
        if name.trim().is_empty() {
            return Err(CoreError::Validation("Informe o nome do usuário".into()));
        }
        let mut user = self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("User not found".into()))?;

        user.email = email;
        user.name = name;
        user.base.updated_at = chrono::Utc::now().naive_utc();
        user.base.synced = false;

        self.repo.update(&user).await?;
        Ok(user)
    }

    /// Atualiza credenciais completas (email + nome + senha opcional),
    /// preservando o `role`. Usado na gestão de login do super admin
    /// (painel de administrador). Valida unicidade do email no tenant
    /// (exceto o próprio). Requer feature `password-hashing`.
    #[cfg(feature = "password-hashing")]
    pub async fn update_credentials(
        &self,
        company_id: Uuid,
        id: Uuid,
        email: String,
        name: String,
        new_password: Option<String>,
        // Foto de perfil: `Some(_)` grava a nova foto (string vazia = remove);
        // `None` mantém a foto atual inalterada (ex.: painel do super admin).
        avatar: Option<String>,
    ) -> Result<User, CoreError> {
        if email.trim().is_empty() {
            return Err(CoreError::Validation("Informe o e-mail do usuário".into()));
        }
        if name.trim().is_empty() {
            return Err(CoreError::Validation("Informe o nome do usuário".into()));
        }
        let mut user = self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("User not found".into()))?;

        // Unicidade do email dentro da empresa (exceto o próprio usuário).
        if let Some(other) = self.repo.find_by_email(company_id, &email).await? {
            if other.base.id != id {
                return Err(CoreError::Validation("E-mail já cadastrado".into()));
            }
        }

        user.email = email;
        user.name = name;
        // Some(_) → grava (vazio remove a foto); None → preserva a atual.
        if let Some(a) = avatar {
            user.avatar = if a.trim().is_empty() { None } else { Some(a) };
        }
        let mut pw_changed = false;
        if let Some(pw) = new_password.filter(|p| !p.trim().is_empty()) {
            user.password_hash = crate::hashing::hash_password(pw).await?;
            pw_changed = true;
        }
        user.base.updated_at = chrono::Utc::now().naive_utc();
        user.base.synced = false;

        self.repo.update(&user).await?;
        // Troca de senha revoga tokens antigos (mudança só de nome/email não).
        if pw_changed {
            self.repo.bump_token_version(company_id, id).await?;
        }
        Ok(user)
    }

    /// Define o telefone de um usuário (pelo e-mail no tenant). Usado no
    /// cadastro de empresa para gravar o telefone do proprietário (admin).
    pub async fn set_phone_by_email(
        &self,
        company_id: Uuid,
        email: &str,
        phone: Option<String>,
    ) -> Result<(), CoreError> {
        if let Some(mut u) = self.repo.find_by_email(company_id, email).await? {
            u.phone = phone.filter(|p| !p.trim().is_empty());
            u.base.updated_at = chrono::Utc::now().naive_utc();
            u.base.synced = false;
            self.repo.update(&u).await?;
        }
        Ok(())
    }

    /// Autentica um usuário por email e senha.
    ///
    /// Retorna o usuário se as credenciais forem válidas.
    /// Mensagem genérica para não vazar existência de emails.
    /// Requer feature `password-hashing` (bcrypt não compila para WASM).
    #[cfg(feature = "password-hashing")]
    pub async fn authenticate(
        &self,
        company_id: Uuid,
        email: &str,
        password: &str,
    ) -> Result<User, CoreError> {
        let user = match self.repo.find_by_email(company_id, email).await? {
            Some(u) => u,
            None => {
                // E-mail inexistente: gasta o mesmo tempo de um verify real
                // (hash dummy) para não vazar existência por latência (§11).
                crate::hashing::verify_dummy(password.to_string()).await;
                return Err(CoreError::Unauthorized("Credenciais inválidas".into()));
            }
        };

        let valid = crate::hashing::verify_password(password.to_string(), user.password_hash.clone()).await?;

        if !valid {
            return Err(CoreError::Unauthorized("Credenciais inválidas".into()));
        }

        Ok(user)
    }

    /// Autentica um usuário por email e senha sem exigir company_id.
    ///
    /// Valida entrada, busca o usuário globalmente pelo email e valida a senha.
    /// Retorna o usuário autenticado (que contém o company_id).
    /// Exceção documentada ao §11 (isolamento por company_id):
    /// necessário para resolver o tenant antes da autenticação no desktop.
    /// Requer feature `password-hashing`.
    #[cfg(feature = "password-hashing")]
    pub async fn authenticate_global(
        &self,
        email: &str,
        password: &str,
    ) -> Result<User, CoreError> {
        if email.trim().is_empty() || password.trim().is_empty() {
            return Err(CoreError::Unauthorized("Credenciais inválidas".into()));
        }

        let user = match self.repo.find_by_email_global(email).await? {
            Some(u) => u,
            None => {
                // E-mail inexistente: equaliza o tempo (hash dummy) — anti
                // enumeração de usuários por latência (§11).
                crate::hashing::verify_dummy(password.to_string()).await;
                return Err(CoreError::Unauthorized("Credenciais inválidas".into()));
            }
        };

        let valid = crate::hashing::verify_password(password.to_string(), user.password_hash.clone()).await?;

        if !valid {
            return Err(CoreError::Unauthorized("Credenciais inválidas".into()));
        }

        Ok(user)
    }

    /// Remoção lógica (soft delete) — genérica (usada também pelo painel
    /// super-admin, que exclui super admins com sua própria guarda).
    pub async fn soft_delete(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("User not found".into()))?;
        self.repo.soft_delete(company_id, id).await
    }

    /// Remoção lógica de um FUNCIONÁRIO (tela de Colaboradores). §11: recusa
    /// alvos que não sejam `Employee` — senão um usuário com `collaborators.edit`
    /// excluiria o Admin do tenant.
    pub async fn delete_employee(&self, company_id: Uuid, id: Uuid) -> Result<(), CoreError> {
        let user = self.repo.find_by_id(company_id, id).await?
            .ok_or_else(|| CoreError::NotFound("Funcionário não encontrado".into()))?;
        if user.role != UserRole::Employee {
            return Err(CoreError::Unauthorized(
                "Só é possível excluir funcionários por aqui".into(),
            ));
        }
        self.repo.soft_delete(company_id, id).await
    }

    /// Busca usuários ainda não sincronizados (§7).
    pub async fn find_unsynced(&self, company_id: Uuid) -> Result<Vec<User>, CoreError> {
        self.repo.find_unsynced(company_id).await
    }

    /// Marca usuário como sincronizado (§7).
    pub async fn mark_synced(&self, company_id: Uuid, id: Uuid, updated_at: chrono::NaiveDateTime) -> Result<(), CoreError> {
        self.repo.mark_synced(company_id, id, updated_at).await
    }

    /// Busca usuários atualizados após o timestamp (§7 — sync pull).
    pub async fn find_updated_since(
        &self,
        company_id: Uuid,
        since: chrono::NaiveDateTime,
    ) -> Result<Vec<User>, CoreError> {
        self.repo.find_updated_since(company_id, since).await
    }

    /// Upsert de sincronização de usuário — caminho CONFIÁVEL (§7.7 — LWW).
    ///
    /// Usado pelo **desktop no pull**, aplicando dados que vieram do servidor
    /// (fonte de verdade). Aqui o `role` é aceito verbatim porque já foi
    /// validado na origem. NÃO use este método para aplicar dados vindos de um
    /// cliente não-confiável — para isso existe [`Self::sync_upsert_from_client`].
    ///
    /// Regras aplicadas (AI_RULES.md §7.7, §11):
    /// - Valida `company_id` contra o tenant.
    /// - Marca `synced = true`; o repository resolve conflito por `updated_at`.
    pub async fn sync_upsert(
        &self,
        company_id: Uuid,
        payload: super::model::SyncUserPayload,
    ) -> Result<(), CoreError> {
        if payload.company_id != company_id {
            return Err(CoreError::Validation("Operação não permitida para esta empresa".into()));
        }
        let mut user = payload.into_user();
        user.base.clamp_future_updated_at();
        user.base.synced = true;
        self.repo.sync_upsert(&user).await
    }

    /// Upsert de usuário vindo de um cliente NÃO-CONFIÁVEL (push desktop →
    /// servidor). §11: o backend nunca confia no `role`/flags do frontend.
    ///
    /// - Valida `company_id` contra o tenant autenticado.
    /// - NUNCA aceita `role == SuperAdmin`: o super admin de plataforma é
    ///   gerido só pelo painel `/admin`, jamais replicado de um desktop de
    ///   tenant (evita escalada cross-tenant — quebra total do multi-tenant).
    /// - Para usuário EXISTENTE, preserva o `role` já persistido no banco —
    ///   um desktop comprometido não promove `employee` → `admin` via sync.
    /// - Para usuário NOVO, só um chamador Admin pode introduzir um Admin.
    /// - `password_hash` é replicado (login multi-dispositivo); conflito por
    ///   `updated_at` no repository.
    pub async fn sync_upsert_from_client(
        &self,
        company_id: Uuid,
        caller_id: Uuid,
        caller_is_admin: bool,
        payload: super::model::SyncUserPayload,
    ) -> Result<(), CoreError> {
        if payload.company_id != company_id {
            return Err(CoreError::Validation("Operação não permitida para esta empresa".into()));
        }
        if payload.role == UserRole::SuperAdmin {
            return Err(CoreError::Validation(
                "Super admin não pode ser sincronizado".into(),
            ));
        }
        let mut user = payload.into_user();
        let mut password_changed = false;
        let mut role_changed = false;
        match self.repo.find_by_id(company_id, user.base.id).await? {
            // Usuário já existe.
            Some(existing) => {
                // Um chamador NÃO-admin só pode sincronizar o PRÓPRIO registro
                // (nome/senha) — nunca o de terceiros (§11: senão sobrescreveria
                // o password_hash de um Admin e faria takeover de conta).
                if !caller_is_admin && user.base.id != caller_id {
                    return Err(CoreError::Forbidden(
                        "Apenas Admin pode alterar credenciais de outro usuário".into(),
                    ));
                }
                // `role` e `job_role_id` são autoridade do servidor: preserva os
                // valores do banco para chamador não-admin — impede auto-escalar
                // permissões trocando o próprio `job_role_id` via sync (§11).
                user.role = existing.role;
                if !caller_is_admin {
                    user.job_role_id = existing.job_role_id;
                }
                // Troca de senha originada no desktop também revoga tokens
                // antigos no servidor (§11) — senão a revogação por versão de
                // credencial teria uma janela offline.
                password_changed = existing.password_hash != user.password_hash;
                // Troca de Função (job_role_id) por um Admin via sync também revoga:
                // as permissões vão DENTRO do JWT, então sem o bump o funcionário
                // rebaixado manteria as permissões antigas até o `exp` (§11).
                // Paridade com `update_employee`/rota REST. (Não-admin não muda o
                // job_role_id — preservado acima —, então aqui é sempre `false`.)
                role_changed = existing.job_role_id != user.job_role_id;
            }
            // Usuário NOVO: só um Admin pode criar usuário via sync (senão um
            // funcionário criaria uma conta com job_role de permissão máxima e
            // senha conhecida — escalada de privilégio, §11).
            None => {
                if !caller_is_admin {
                    return Err(CoreError::Forbidden(
                        "Apenas Admin pode criar usuário".into(),
                    ));
                }
                // Mesmo isolamento do `create_employee`: o e-mail não pode já
                // pertencer a OUTRA empresa. Sem isto, um admin do tenant A
                // criava por sync um usuário com o e-mail do dono do tenant B
                // e travava o login global dele (DoS cross-tenant). `Err` do
                // global = já em duas empresas; barra também.
                match self.repo.find_by_email_global(&user.email).await {
                    Ok(Some(u))
                        if u.base.company_id != company_id && u.base.deleted_at.is_none() =>
                    {
                        return Err(CoreError::Validation(
                            "E-mail já cadastrado em outra empresa".into(),
                        ));
                    }
                    Err(_) => {
                        return Err(CoreError::Validation(
                            "E-mail já cadastrado em outra empresa".into(),
                        ));
                    }
                    _ => {}
                }
            }
        }
        user.base.clamp_future_updated_at();
        user.base.synced = true;
        self.repo.sync_upsert(&user).await?;
        if password_changed || role_changed {
            self.repo.bump_token_version(company_id, user.base.id).await?;
        }
        Ok(())
    }
}
