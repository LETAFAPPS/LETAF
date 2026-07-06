-- Funções (cargos) com permissões granulares — RBAC de colaboradores.
-- Espelha desktop/064.
--
-- Regras (AI_RULES.md §6, §7, §11): BaseFields + soft delete + synced;
-- isolamento por company_id; permissões validadas no backend.
CREATE TABLE job_roles (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id),
    name TEXT NOT NULL,
    -- JSON array de chaves "feature.action" (ver core::permission).
    permissions TEXT NOT NULL DEFAULT '[]',
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL,
    updated_at TIMESTAMP WITHOUT TIME ZONE NOT NULL,
    deleted_at TIMESTAMP WITHOUT TIME ZONE,
    synced BOOLEAN NOT NULL DEFAULT FALSE
);
CREATE INDEX idx_job_roles_company ON job_roles(company_id);
CREATE INDEX idx_job_roles_sync ON job_roles(company_id, updated_at);

-- Funcionário (user) pode ter uma função atribuída. Admin/SuperAdmin
-- não dependem de função (acesso total). NULL = sem função.
ALTER TABLE users ADD COLUMN job_role_id UUID REFERENCES job_roles(id);
