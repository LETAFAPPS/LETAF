-- Categorias de lançamentos financeiros (Fase 11).
-- Isolamento multi-tenant por company_id; soft delete + sync.
CREATE TABLE finance_categories (
    id TEXT PRIMARY KEY,
    company_id TEXT NOT NULL,
    name TEXT NOT NULL,
    color TEXT NOT NULL DEFAULT '',
    icon TEXT NOT NULL DEFAULT '',
    -- 'payable' | 'receivable' | 'both'
    scope TEXT NOT NULL DEFAULT 'both',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT,
    synced INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_finance_categories_company ON finance_categories(company_id);
CREATE INDEX idx_finance_categories_company_synced ON finance_categories(company_id, synced);
CREATE INDEX idx_finance_categories_company_scope ON finance_categories(company_id, scope);
