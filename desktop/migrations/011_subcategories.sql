-- Tabela de subcategorias no SQLite (AI_RULES.md §5, §6, §11).
CREATE TABLE IF NOT EXISTS subcategories (
    id          TEXT PRIMARY KEY,
    company_id  TEXT NOT NULL,
    category_id TEXT NOT NULL,
    name        TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    deleted_at  TEXT,
    synced      INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_subcategories_company_id ON subcategories(company_id);
CREATE INDEX IF NOT EXISTS idx_subcategories_category_id ON subcategories(company_id, category_id);
