-- Carteira do cliente (Fase 2): 1 conta por (company_id, customer_id).
-- `balance` pode ser negativo (fiado); `credit_limit >= 0` controla
-- até onde o saldo pode descer (service valida).
CREATE TABLE wallet_accounts (
    id TEXT PRIMARY KEY,
    company_id TEXT NOT NULL,
    customer_id TEXT NOT NULL,
    balance REAL NOT NULL DEFAULT 0,
    credit_limit REAL NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT,
    synced INTEGER NOT NULL DEFAULT 0
);

-- 1:1 com customer dentro do tenant — service garante via
-- `find_account_by_customer` (idempotent upsert no `open_account`).
CREATE UNIQUE INDEX idx_wallet_accounts_company_customer
    ON wallet_accounts(company_id, customer_id)
    WHERE deleted_at IS NULL;
CREATE INDEX idx_wallet_accounts_company ON wallet_accounts(company_id);
CREATE INDEX idx_wallet_accounts_company_synced ON wallet_accounts(company_id, synced);
