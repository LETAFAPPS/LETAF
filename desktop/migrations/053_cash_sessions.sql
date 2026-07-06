-- Sessões de caixa (abertura → fechamento).
-- Apenas UMA sessão Open por (company_id) a cada momento — regra do
-- service, não constraint do banco (pra não inviabilizar correções
-- manuais em casos de bug).

CREATE TABLE cash_sessions (
    id TEXT PRIMARY KEY,
    company_id TEXT NOT NULL,
    operator_id TEXT NOT NULL,
    operator_name TEXT NOT NULL,
    opened_at TEXT NOT NULL,
    closed_at TEXT,
    initial_change REAL NOT NULL DEFAULT 0,
    counted_cash REAL,
    status TEXT NOT NULL DEFAULT 'open',
    open_notes TEXT,
    close_notes TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT,
    synced INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_cash_sessions_company ON cash_sessions(company_id);
CREATE INDEX idx_cash_sessions_company_status ON cash_sessions(company_id, status);
CREATE INDEX idx_cash_sessions_company_synced ON cash_sessions(company_id, synced);
CREATE INDEX idx_cash_sessions_company_opened_at ON cash_sessions(company_id, opened_at DESC);
