-- Movimentos da carteira — espelha desktop/058.
CREATE TABLE wallet_movements (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id),
    account_id UUID NOT NULL,
    kind TEXT NOT NULL,
    amount NUMERIC(14, 2) NOT NULL,
    balance_after NUMERIC(14, 2) NOT NULL,
    related_order_id UUID,
    notes TEXT,
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL,
    updated_at TIMESTAMP WITHOUT TIME ZONE NOT NULL,
    deleted_at TIMESTAMP WITHOUT TIME ZONE,
    synced BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE INDEX idx_wallet_movements_company ON wallet_movements(company_id);
CREATE INDEX idx_wallet_movements_account ON wallet_movements(account_id);
CREATE INDEX idx_wallet_movements_account_created
    ON wallet_movements(account_id, created_at DESC);
