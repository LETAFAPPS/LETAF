-- Livro-razão da carteira (Fase 2). Append-only no service exceto
-- atualização do flag `synced`. `balance_after` é snapshot para
-- auditoria sem precisar replay.
CREATE TABLE wallet_movements (
    id TEXT PRIMARY KEY,
    company_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    -- 'deposit' | 'withdraw' | 'order_charge' | 'order_refund' | 'manual_adjust'
    kind TEXT NOT NULL,
    amount REAL NOT NULL,
    balance_after REAL NOT NULL,
    related_order_id TEXT,
    notes TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT,
    synced INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_wallet_movements_company ON wallet_movements(company_id);
CREATE INDEX idx_wallet_movements_account ON wallet_movements(account_id);
CREATE INDEX idx_wallet_movements_company_synced ON wallet_movements(company_id, synced);
CREATE INDEX idx_wallet_movements_account_created
    ON wallet_movements(account_id, created_at DESC);
