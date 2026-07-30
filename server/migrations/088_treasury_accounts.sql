-- Carteira do estabelecimento (tesouraria) — espelha desktop/080.
-- SINGLETON por empresa (UNIQUE em company_id); dinheiro em NUMERIC
-- (exato, §13). `initial_balance` validado >= 0 no service.
CREATE TABLE treasury_accounts (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id),
    initial_balance NUMERIC(14,2) NOT NULL DEFAULT 0,
    notes TEXT,
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL,
    updated_at TIMESTAMP WITHOUT TIME ZONE NOT NULL,
    deleted_at TIMESTAMP WITHOUT TIME ZONE,
    synced BOOLEAN NOT NULL DEFAULT FALSE,
    UNIQUE(company_id)
);

CREATE INDEX idx_treasury_accounts_company_updated
    ON treasury_accounts(company_id, updated_at);
