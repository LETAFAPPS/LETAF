-- Tabela de categorias (AI_RULES.md §6).
--
-- Campos obrigatórios:
--   id            UUID PK (sem auto-incremento)
--   company_id    FK → companies (isolamento multi-tenant)
--   created_at    TIMESTAMP NOT NULL
--   updated_at    TIMESTAMP NOT NULL
--   deleted_at    TIMESTAMP NULL (soft delete)
--   synced        BOOLEAN NOT NULL DEFAULT false
--
-- Campos de domínio: name, description.

CREATE TABLE IF NOT EXISTS categories (
    id          UUID PRIMARY KEY,
    company_id  UUID NOT NULL REFERENCES companies(id),
    name        TEXT NOT NULL,
    description TEXT,
    created_at  TIMESTAMP NOT NULL DEFAULT now(),
    updated_at  TIMESTAMP NOT NULL DEFAULT now(),
    deleted_at  TIMESTAMP,
    synced      BOOLEAN NOT NULL DEFAULT false
);

CREATE INDEX IF NOT EXISTS idx_categories_company_id ON categories(company_id);

-- Adiciona FK category_id em products (nullable)
ALTER TABLE products ADD COLUMN IF NOT EXISTS category_id UUID REFERENCES categories(id);
