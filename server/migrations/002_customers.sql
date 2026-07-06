-- Migration — tabela customers (PostgreSQL)
--
-- Regras aplicadas (AI_RULES.md §6):
-- - id: UUID (sem auto-incremento)
-- - company_id: isolamento multi-tenant
-- - created_at / updated_at: timestamps obrigatórios
-- - deleted_at: soft delete
-- - synced: controle de sincronização

CREATE TABLE customers (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id),
    name TEXT NOT NULL,
    email TEXT,
    phone TEXT,
    document TEXT,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    deleted_at TIMESTAMP,
    synced BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE INDEX idx_customers_company_id ON customers(company_id);
