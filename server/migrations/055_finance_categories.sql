-- Categorias de lançamentos financeiros — espelha desktop/055.
CREATE TABLE finance_categories (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id),
    name TEXT NOT NULL,
    color TEXT NOT NULL DEFAULT '',
    icon TEXT NOT NULL DEFAULT '',
    scope TEXT NOT NULL DEFAULT 'both',
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL,
    updated_at TIMESTAMP WITHOUT TIME ZONE NOT NULL,
    deleted_at TIMESTAMP WITHOUT TIME ZONE,
    synced BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE INDEX idx_finance_categories_company ON finance_categories(company_id);
CREATE INDEX idx_finance_categories_company_scope ON finance_categories(company_id, scope);
