-- Migration inicial — PostgreSQL (server)
--
-- Regras aplicadas (AI_RULES.md §6):
-- - id: UUID (sem auto-incremento)
-- - company_id: isolamento multi-tenant
-- - created_at / updated_at: timestamps obrigatórios
-- - deleted_at: soft delete
-- - synced: controle de sincronização

CREATE TABLE companies (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    subdomain TEXT NOT NULL UNIQUE,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    deleted_at TIMESTAMP,
    synced BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE TABLE users (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id),
    email TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    name TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    deleted_at TIMESTAMP,
    synced BOOLEAN NOT NULL DEFAULT FALSE,
    UNIQUE (company_id, email)
);

CREATE TABLE products (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id),
    name TEXT NOT NULL,
    description TEXT,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    deleted_at TIMESTAMP,
    synced BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE INDEX idx_users_company_id ON users(company_id);
CREATE INDEX idx_products_company_id ON products(company_id);
CREATE INDEX idx_companies_subdomain ON companies(subdomain);
