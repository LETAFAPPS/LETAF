-- Tabela de fornecedores (AI_RULES.md §6).
--
-- Campos obrigatórios:
--   id            UUID PK (sem auto-incremento)
--   company_id    FK → companies (isolamento multi-tenant)
--   created_at    TIMESTAMP NOT NULL
--   updated_at    TIMESTAMP NOT NULL
--   deleted_at    TIMESTAMP NULL (soft delete)
--   synced        BOOLEAN NOT NULL DEFAULT false
--
-- Campos de domínio: name, email, phone, document.

CREATE TABLE IF NOT EXISTS suppliers (
    id          UUID PRIMARY KEY,
    company_id  UUID NOT NULL REFERENCES companies(id),
    name        TEXT NOT NULL,
    email       TEXT,
    phone       TEXT,
    document    TEXT,
    created_at  TIMESTAMP NOT NULL DEFAULT now(),
    updated_at  TIMESTAMP NOT NULL DEFAULT now(),
    deleted_at  TIMESTAMP,
    synced      BOOLEAN NOT NULL DEFAULT false
);
