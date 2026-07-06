-- Livro-razão de uma sessão de caixa.
-- kind: 'opening' | 'sale' | 'sangria' | 'suprimento'
-- amount sempre positivo (sinal aplicado pelo kind no service).
-- method só preenchido em sales (cash/credit/debit/pix).
-- order_id preenchido só em sales — FK lógica, sem CONSTRAINT REFERENCES
-- pra evitar circular com migrations antigas e simplificar sync.

CREATE TABLE cash_movements (
    id TEXT PRIMARY KEY,
    company_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    amount REAL NOT NULL,
    method TEXT,
    reason TEXT NOT NULL DEFAULT '',
    detail TEXT,
    order_id TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT,
    synced INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_cash_movements_company ON cash_movements(company_id);
CREATE INDEX idx_cash_movements_session ON cash_movements(session_id);
CREATE INDEX idx_cash_movements_company_synced ON cash_movements(company_id, synced);
