-- Sessões de caixa — espelha desktop/migrations/053.
CREATE TABLE cash_sessions (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id),
    operator_id UUID NOT NULL,
    operator_name TEXT NOT NULL,
    opened_at TIMESTAMP WITHOUT TIME ZONE NOT NULL,
    closed_at TIMESTAMP WITHOUT TIME ZONE,
    initial_change DOUBLE PRECISION NOT NULL DEFAULT 0,
    counted_cash DOUBLE PRECISION,
    status TEXT NOT NULL DEFAULT 'open',
    open_notes TEXT,
    close_notes TEXT,
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL,
    updated_at TIMESTAMP WITHOUT TIME ZONE NOT NULL,
    deleted_at TIMESTAMP WITHOUT TIME ZONE,
    synced BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE INDEX idx_cash_sessions_company ON cash_sessions(company_id);
CREATE INDEX idx_cash_sessions_company_status ON cash_sessions(company_id, status);
CREATE INDEX idx_cash_sessions_company_opened_at ON cash_sessions(company_id, opened_at DESC);
