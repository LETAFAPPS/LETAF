-- Tabela de subcategorias (AI_RULES.md §6, §11).
--
-- Cada subcategoria pertence a UMA categoria e à mesma empresa.
-- Isolamento multi-tenant: toda consulta filtra por company_id (§11).

CREATE TABLE IF NOT EXISTS subcategories (
    id          UUID PRIMARY KEY,
    company_id  UUID NOT NULL REFERENCES companies(id),
    category_id UUID NOT NULL REFERENCES categories(id),
    name        TEXT NOT NULL,
    created_at  TIMESTAMP NOT NULL DEFAULT now(),
    updated_at  TIMESTAMP NOT NULL DEFAULT now(),
    deleted_at  TIMESTAMP,
    synced      BOOLEAN NOT NULL DEFAULT false
);

CREATE INDEX IF NOT EXISTS idx_subcategories_company_id ON subcategories(company_id);
CREATE INDEX IF NOT EXISTS idx_subcategories_category_id ON subcategories(company_id, category_id);
