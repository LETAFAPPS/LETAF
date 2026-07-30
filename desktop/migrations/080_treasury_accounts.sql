-- Carteira do estabelecimento (tesouraria): SINGLETON por empresa
-- (UNIQUE em company_id). `initial_balance` é o saldo inicial declarado
-- pelo operador ao abrir a carteira (service valida >= 0).
-- Dinheiro no SQLite = REAL (§13). Espelha server/088_treasury_accounts.sql.
CREATE TABLE treasury_accounts (
    id TEXT PRIMARY KEY,
    company_id TEXT NOT NULL,
    initial_balance REAL NOT NULL DEFAULT 0,
    notes TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT,
    synced INTEGER NOT NULL DEFAULT 0,
    UNIQUE(company_id)
);

CREATE INDEX idx_treasury_accounts_company_synced
    ON treasury_accounts(company_id, synced);
