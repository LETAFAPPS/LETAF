-- Tabela de fornecedores no SQLite (AI_RULES.md §5, §6).
--
-- Mesma estrutura do PostgreSQL, adaptada para SQLite:
--   - UUID armazenado como TEXT
--   - TIMESTAMP armazenado como TEXT (ISO 8601)
--   - BOOLEAN armazenado como INTEGER (0/1)

CREATE TABLE IF NOT EXISTS suppliers (
    id          TEXT PRIMARY KEY,
    company_id  TEXT NOT NULL,
    name        TEXT NOT NULL,
    email       TEXT,
    phone       TEXT,
    document    TEXT,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    deleted_at  TEXT,
    synced      INTEGER NOT NULL DEFAULT 0
);
