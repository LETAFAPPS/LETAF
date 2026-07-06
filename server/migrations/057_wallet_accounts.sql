-- Carteira do cliente — espelha desktop/057.
CREATE TABLE wallet_accounts (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id),
    customer_id UUID NOT NULL,
    balance NUMERIC(14, 2) NOT NULL DEFAULT 0,
    credit_limit NUMERIC(14, 2) NOT NULL DEFAULT 0,
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL,
    updated_at TIMESTAMP WITHOUT TIME ZONE NOT NULL,
    deleted_at TIMESTAMP WITHOUT TIME ZONE,
    synced BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE UNIQUE INDEX idx_wallet_accounts_company_customer
    ON wallet_accounts(company_id, customer_id)
    WHERE deleted_at IS NULL;
CREATE INDEX idx_wallet_accounts_company ON wallet_accounts(company_id);
