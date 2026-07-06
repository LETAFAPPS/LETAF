-- Funções (cargos) com permissões granulares — RBAC. Espelha server/064.
CREATE TABLE job_roles (
    id TEXT PRIMARY KEY,
    company_id TEXT NOT NULL,
    name TEXT NOT NULL,
    -- JSON array de chaves "feature.action" (ver core::permission).
    permissions TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT,
    synced INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_job_roles_company ON job_roles(company_id);
CREATE INDEX idx_job_roles_sync ON job_roles(company_id, updated_at);

-- Funcionário (user) pode ter uma função atribuída (Admin não precisa).
ALTER TABLE users ADD COLUMN job_role_id TEXT;
