-- Migration — tabela customers (SQLite)
--
-- Regras aplicadas (AI_RULES.md §6):
-- - id: UUID como TEXT (sem auto-incremento)
-- - company_id: isolamento (fixo no desktop)
-- - created_at / updated_at: timestamps como TEXT
-- - deleted_at: soft delete
-- - synced: INTEGER (0/1)

CREATE TABLE IF NOT EXISTS customers (
    id TEXT PRIMARY KEY,
    company_id TEXT NOT NULL REFERENCES companies(id),
    name TEXT NOT NULL,
    email TEXT,
    phone TEXT,
    document TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT,
    synced INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_customers_company_id ON customers(company_id);
