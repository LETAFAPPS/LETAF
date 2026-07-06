-- Tabela de categorias no SQLite (AI_RULES.md §5, §6).

CREATE TABLE IF NOT EXISTS categories (
    id          TEXT PRIMARY KEY,
    company_id  TEXT NOT NULL,
    name        TEXT NOT NULL,
    description TEXT,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    deleted_at  TEXT,
    synced      INTEGER NOT NULL DEFAULT 0
);

-- Adiciona category_id em products (nullable)
ALTER TABLE products ADD COLUMN category_id TEXT;
