-- Livro-razão de caixa — espelha desktop/migrations/054.
CREATE TABLE cash_movements (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id),
    session_id UUID NOT NULL REFERENCES cash_sessions(id),
    kind TEXT NOT NULL,
    amount NUMERIC(14, 2) NOT NULL,
    method TEXT,
    reason TEXT NOT NULL DEFAULT '',
    detail TEXT,
    order_id UUID,
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL,
    updated_at TIMESTAMP WITHOUT TIME ZONE NOT NULL,
    deleted_at TIMESTAMP WITHOUT TIME ZONE,
    synced BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE INDEX idx_cash_movements_company ON cash_movements(company_id);
CREATE INDEX idx_cash_movements_session ON cash_movements(session_id);
